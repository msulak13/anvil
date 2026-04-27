//! Dependency graph construction and validation.
//!
//! M3. Given the IR a parser produced for an entire project, this module
//! materializes the per-component dependency graph and runs all v0.1
//! validation rules over it.
//!
//! # Pipeline
//!
//! 1. [`build_and_validate`] takes a [`GraphInput`] (one component plus the
//!    project's full set of `@Module` declarations and `@Inject` self-bindings).
//! 2. It aggregates the component's bindings — `@Provides` from included
//!    modules plus all `@Inject` self-bindings — into a [`DependencyGraph`].
//! 3. It runs the four diagnostic rules:
//!    - **Duplicate**: two bindings declared for the same key.
//!    - **Missing**: a dep (or entry-point) with no declared binding.
//!    - **Cycle**: a strongly-connected component of size ≥ 2 (Tarjan SCC).
//!    - **`ScopeMismatch`**: a [`Scope::Singleton`] binding inside a
//!      non-singleton component.
//! 4. It returns the (best-effort) graph plus a `Vec<Diagnostic>`. The
//!    graph is always populated even when diagnostics are non-empty so
//!    downstream tooling (e.g. `tsdi explain`) can introspect partial
//!    state.
//!
//! Rendering the diagnostics for human display is `tsdi-cli`'s job; this
//! crate emits structured [`Diagnostic`] data only.

use std::collections::{HashMap, HashSet};

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::ir::{Binding, ClassRef, ComponentDecl, Key, ModuleDecl, Provider, Scope, SourceSpan};
use crate::validate::{Diagnostic, DiagnosticKind, Label};

/// Inputs to [`build_and_validate`].
///
/// The caller — typically `tsdi-cli` — is responsible for collecting these
/// from the parser's [`crate::ir::ParsedFile`] outputs.
#[derive(Clone, Copy, Debug)]
pub struct GraphInput<'a> {
    /// The component whose graph is being built.
    pub component: &'a ComponentDecl,
    /// Every `@Module` declaration the parser saw across the project.
    /// Only modules listed in `component.modules` actually contribute
    /// bindings; the rest are ignored.
    pub modules: &'a [ModuleDecl],
    /// Every `@Inject`-annotated class self-binding the parser saw across
    /// the project. All are eligible to satisfy a key.
    pub inject_classes: &'a [Binding],
}

/// A resolved per-component dependency graph.
///
/// Node identity is the [`Key`]. Edges go from a binding to each of its
/// dependencies. The structure is exposed so that `tsdi-codegen` and the
/// `tsdi explain` debug subcommand can walk it without re-deriving it.
#[derive(Clone, Debug)]
pub struct DependencyGraph {
    /// The component this graph belongs to.
    pub component: ClassRef,
    /// The component's own scope.
    pub component_scope: Scope,
    /// Entry-point keys, in source order.
    pub roots: Vec<Key>,
    /// All bindings reachable inside this component, indexed by key.
    pub bindings: HashMap<Key, Binding>,
    /// Underlying directed graph with `Key` nodes.
    pub graph: DiGraph<Key, ()>,
    /// Lookup from key to its node index in `graph`.
    pub node_for: HashMap<Key, NodeIndex>,
}

impl DependencyGraph {
    /// Number of bindings in the graph.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.bindings.len()
    }

    /// Total number of dependency edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

/// Build the dependency graph for `input.component` and run all M3
/// validation rules.
///
/// Always returns a (possibly partial) [`DependencyGraph`]. Callers should
/// inspect the returned `Vec<Diagnostic>` first: a non-empty list means
/// the graph is unsafe to feed to codegen.
#[must_use]
pub fn build_and_validate(input: GraphInput<'_>) -> (DependencyGraph, Vec<Diagnostic>) {
    let (bindings, mut diagnostics) = aggregate_bindings(input);
    let (graph, node_for, missing) = build_petgraph(&bindings);
    diagnostics.extend(missing);

    let mut roots: Vec<Key> = Vec::with_capacity(input.component.entry_points.len());
    for ep in &input.component.entry_points {
        roots.push(ep.key.clone());
        if !node_for.contains_key(&ep.key) {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::MissingBinding {
                    key: ep.key.clone(),
                    requested_by: None,
                },
                primary: Label {
                    span: ep.source.clone(),
                    message: format!("entry point `{}` has no binding", ep.name),
                },
                related: vec![],
            });
        }
    }

    diagnostics.extend(detect_cycles(&graph, &bindings, input.component));
    diagnostics.extend(detect_scope_mismatches(&bindings, input.component));

    let dep_graph = DependencyGraph {
        component: input.component.class.clone(),
        component_scope: input.component.scope,
        roots,
        bindings,
        graph,
        node_for,
    };
    (dep_graph, diagnostics)
}

/// Collect bindings from the component's modules + project-wide self-bindings,
/// emitting a [`DiagnosticKind::Duplicate`] for any key declared more than once.
fn aggregate_bindings(input: GraphInput<'_>) -> (HashMap<Key, Binding>, Vec<Diagnostic>) {
    let mut bindings: HashMap<Key, Binding> = HashMap::new();
    let mut sources_for_key: HashMap<Key, Vec<SourceSpan>> = HashMap::new();
    let component_modules: HashSet<&ClassRef> = input.component.modules.iter().collect();

    for m in input.modules {
        if !component_modules.contains(&m.class) {
            continue;
        }
        for b in &m.provides {
            register_binding(b, &mut bindings, &mut sources_for_key);
        }
    }
    for b in input.inject_classes {
        register_binding(b, &mut bindings, &mut sources_for_key);
    }

    let mut diagnostics = Vec::new();
    for (key, spans) in &sources_for_key {
        if spans.len() <= 1 {
            continue;
        }
        let mut iter = spans.iter();
        let primary = iter.next().expect("len > 1").clone();
        let related: Vec<Label> = iter
            .map(|s| Label {
                span: s.clone(),
                message: "also declared here".to_owned(),
            })
            .collect();
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Duplicate { key: key.clone() },
            primary: Label {
                span: primary,
                message: "first declaration".to_owned(),
            },
            related,
        });
    }

    (bindings, diagnostics)
}

/// Build the petgraph DAG over `bindings` and return any missing-dep
/// diagnostics encountered while wiring edges.
fn build_petgraph(
    bindings: &HashMap<Key, Binding>,
) -> (DiGraph<Key, ()>, HashMap<Key, NodeIndex>, Vec<Diagnostic>) {
    let mut graph: DiGraph<Key, ()> = DiGraph::new();
    let mut node_for: HashMap<Key, NodeIndex> = HashMap::new();
    for key in bindings.keys() {
        let ix = graph.add_node(key.clone());
        node_for.insert(key.clone(), ix);
    }

    let mut missing = Vec::new();
    for (key, binding) in bindings {
        let from = node_for[key];
        for dep in &binding.deps {
            match node_for.get(dep) {
                Some(&to) => {
                    graph.add_edge(from, to, ());
                }
                None => missing.push(missing_for_dep(dep.clone(), key.clone(), binding)),
            }
        }
    }
    (graph, node_for, missing)
}

/// Run Tarjan SCC and emit one [`DiagnosticKind::Cycle`] per non-trivial
/// strongly-connected component (size ≥ 2 or self-loop).
fn detect_cycles(
    graph: &DiGraph<Key, ()>,
    bindings: &HashMap<Key, Binding>,
    component: &ComponentDecl,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for scc in tarjan_scc(graph) {
        let is_self_loop = scc.len() == 1 && graph.contains_edge(scc[0], scc[0]);
        if scc.len() < 2 && !is_self_loop {
            continue;
        }
        let keys: Vec<Key> = scc.iter().map(|&ix| graph[ix].clone()).collect();
        let primary_span = bindings
            .get(&keys[0])
            .map_or_else(|| component.source.clone(), |b| b.source.clone());
        let related: Vec<Label> = keys
            .iter()
            .skip(1)
            .filter_map(|k| {
                bindings.get(k).map(|b| Label {
                    span: b.source.clone(),
                    message: "in cycle".to_owned(),
                })
            })
            .collect();
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Cycle { keys },
            primary: Label {
                span: primary_span,
                message: "cycle detected starting here".to_owned(),
            },
            related,
        });
    }
    diagnostics
}

/// Emit one [`DiagnosticKind::ScopeMismatch`] per `Singleton` binding
/// inside a non-`Singleton` component.
fn detect_scope_mismatches(
    bindings: &HashMap<Key, Binding>,
    component: &ComponentDecl,
) -> Vec<Diagnostic> {
    if component.scope == Scope::Singleton {
        return Vec::new();
    }
    bindings
        .values()
        .filter(|b| b.scope == Scope::Singleton)
        .map(|b| Diagnostic {
            kind: DiagnosticKind::ScopeMismatch {
                key: b.key.clone(),
                binding_scope: b.scope,
                component_scope: component.scope,
            },
            primary: Label {
                span: b.source.clone(),
                message: "singleton binding".to_owned(),
            },
            related: vec![Label {
                span: component.source.clone(),
                message: "component is not @Singleton".to_owned(),
            }],
        })
        .collect()
}

/// Insert a binding, recording the declaration site so duplicate detection
/// can replay every contributor.
fn register_binding(
    b: &Binding,
    bindings: &mut HashMap<Key, Binding>,
    sources_for_key: &mut HashMap<Key, Vec<SourceSpan>>,
) {
    sources_for_key
        .entry(b.key.clone())
        .or_default()
        .push(b.source.clone());
    bindings.entry(b.key.clone()).or_insert_with(|| b.clone());
}

fn missing_for_dep(missing: Key, requested_by: Key, binding: &Binding) -> Diagnostic {
    let provider_kind = match &binding.provider {
        Provider::InjectCtor { .. } => "@Inject ctor",
        Provider::ProvidesMethod { .. } => "@Provides method",
        Provider::Binds { .. } => "@Binds method",
    };
    Diagnostic {
        kind: DiagnosticKind::MissingBinding {
            key: missing,
            requested_by: Some(requested_by),
        },
        primary: Label {
            span: binding.source.clone(),
            message: format!("dep requested by this {provider_kind}"),
        },
        related: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        Binding, ClassRef, ComponentDecl, EntryPoint, Key, ModuleDecl, ModulePath, Provider, Scope,
        SourceSpan,
    };

    fn key(name: &str) -> Key {
        Key::Class {
            module: ModulePath(format!("/p/{name}.ts")),
            name: name.to_owned(),
        }
    }

    fn class_ref(name: &str) -> ClassRef {
        ClassRef {
            module: ModulePath(format!("/p/{name}.ts")),
            name: name.to_owned(),
        }
    }

    fn span(name: &str, start: u32) -> SourceSpan {
        SourceSpan::new(format!("/p/{name}.ts"), start, start + 10)
    }

    fn inject(name: &str, deps: Vec<Key>, scope: Scope) -> Binding {
        Binding {
            key: key(name),
            provider: Provider::InjectCtor {
                class: class_ref(name),
            },
            scope,
            deps,
            source: span(name, 0),
        }
    }

    fn provides(module_name: &str, ret: &str, deps: Vec<Key>, scope: Scope) -> Binding {
        Binding {
            key: key(ret),
            provider: Provider::ProvidesMethod {
                module: class_ref(module_name),
                method: format!("provide{ret}"),
            },
            scope,
            deps,
            source: span(module_name, 100),
        }
    }

    fn empty_component(name: &str, modules: Vec<ClassRef>, scope: Scope) -> ComponentDecl {
        ComponentDecl {
            class: class_ref(name),
            modules,
            scope,
            entry_points: vec![],
            source: span(name, 0),
        }
    }

    fn ep(name: &str, k: Key) -> EntryPoint {
        EntryPoint {
            name: name.to_owned(),
            key: k,
            source: span("Component", 50),
        }
    }

    #[test]
    fn empty_component_with_no_entry_points_is_valid() {
        let comp = empty_component("Comp", vec![], Scope::Unscoped);
        let (g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[],
        });
        assert!(ds.is_empty(), "diagnostics: {ds:?}");
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn happy_path_chain_resolves() {
        // Pump -> Heater (both @Inject)
        let heater = inject("Heater", vec![], Scope::Unscoped);
        let pump = inject("Pump", vec![key("Heater")], Scope::Unscoped);

        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("pump", key("Pump")));

        let (g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[heater, pump],
        });
        assert!(ds.is_empty(), "diagnostics: {ds:?}");
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn missing_binding_for_dep_is_reported() {
        // Pump depends on Heater, but Heater has no binding.
        let pump = inject("Pump", vec![key("Heater")], Scope::Unscoped);
        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("pump", key("Pump")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[pump],
        });
        assert_eq!(ds.len(), 1);
        match &ds[0].kind {
            DiagnosticKind::MissingBinding { key, requested_by } => {
                assert_eq!(key.clone(), super::tests::key("Heater"));
                assert_eq!(requested_by.clone(), Some(super::tests::key("Pump")));
            }
            other => panic!("unexpected diagnostic: {other:?}"),
        }
    }

    #[test]
    fn missing_binding_for_entry_point_is_reported() {
        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("missing", key("NoSuch")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[],
        });
        assert_eq!(ds.len(), 1);
        match &ds[0].kind {
            DiagnosticKind::MissingBinding { requested_by, .. } => {
                assert!(requested_by.is_none(), "entry-point miss has no requester");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cycle_is_detected() {
        // A -> B -> A
        let a = inject("A", vec![key("B")], Scope::Unscoped);
        let b = inject("B", vec![key("A")], Scope::Unscoped);
        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("a", key("A")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[a, b],
        });
        let cycles: Vec<&Diagnostic> = ds
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::Cycle { .. }))
            .collect();
        assert_eq!(cycles.len(), 1, "diagnostics: {ds:?}");
    }

    #[test]
    fn self_loop_is_detected_as_cycle() {
        let a = inject("A", vec![key("A")], Scope::Unscoped);
        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("a", key("A")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[a],
        });
        assert!(
            ds.iter()
                .any(|d| matches!(d.kind, DiagnosticKind::Cycle { .. })),
            "{ds:?}"
        );
    }

    #[test]
    fn duplicate_binding_is_detected() {
        let m1 = ModuleDecl {
            class: class_ref("M1"),
            provides: vec![provides("M1", "Heater", vec![], Scope::Unscoped)],
            source: span("M1", 0),
        };
        let m2 = ModuleDecl {
            class: class_ref("M2"),
            provides: vec![provides("M2", "Heater", vec![], Scope::Unscoped)],
            source: span("M2", 0),
        };
        let comp = empty_component(
            "Comp",
            vec![class_ref("M1"), class_ref("M2")],
            Scope::Unscoped,
        );

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[m1, m2],
            inject_classes: &[],
        });
        assert_eq!(ds.len(), 1);
        match &ds[0].kind {
            DiagnosticKind::Duplicate { key } => {
                assert_eq!(key.clone(), super::tests::key("Heater"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ds[0].related.len(), 1);
    }

    #[test]
    fn scope_mismatch_singleton_in_unscoped_component() {
        let heater = inject("Heater", vec![], Scope::Singleton);
        let mut comp = empty_component("Comp", vec![], Scope::Unscoped);
        comp.entry_points.push(ep("h", key("Heater")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[heater],
        });
        assert!(
            ds.iter()
                .any(|d| matches!(d.kind, DiagnosticKind::ScopeMismatch { .. })),
            "{ds:?}"
        );
    }

    #[test]
    fn singleton_binding_in_singleton_component_is_ok() {
        let heater = inject("Heater", vec![], Scope::Singleton);
        let mut comp = empty_component("Comp", vec![], Scope::Singleton);
        comp.entry_points.push(ep("h", key("Heater")));

        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[heater],
        });
        assert!(ds.is_empty(), "{ds:?}");
    }

    #[test]
    fn module_not_referenced_by_component_is_ignored() {
        let m_unused = ModuleDecl {
            class: class_ref("Unused"),
            provides: vec![provides("Unused", "Ghost", vec![], Scope::Unscoped)],
            source: span("Unused", 0),
        };
        let comp = empty_component("Comp", vec![], Scope::Unscoped);
        let (g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[m_unused],
            inject_classes: &[],
        });
        assert!(ds.is_empty());
        assert_eq!(g.node_count(), 0);
    }
}
