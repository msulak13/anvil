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

use crate::ir::{
    Binding, ClassRef, ComponentDecl, EntryPoint, FactoryParam, Key, ModuleDecl, MultibindRole,
    Provider, Scope, SetContributor, SourceSpan, SubcomponentDecl,
};
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
    /// Every `@Subcomponent` declaration the parser saw. Used to recognize
    /// entry-point keys whose return type names a subcomponent class —
    /// those become subcomponent **factories**, not regular bindings.
    pub subcomponents: &'a [SubcomponentDecl],
}

/// A subcomponent factory entry point on a parent component.
///
/// Produced when one of the parent's entry-point methods has a return type
/// matching a [`SubcomponentDecl`]. The child graph is built recursively
/// with parent bindings available as fallback (so inherited deps route
/// through `this.parent.<getX>()` at codegen time).
#[derive(Clone, Debug)]
pub struct SubcomponentFactory {
    /// The parent's abstract method name that exposes this child.
    pub method_name: String,
    /// The subcomponent class itself.
    pub subcomponent: ClassRef,
    /// Fully validated child graph.
    pub child_graph: DependencyGraph,
    /// The subcomponent's own entry points (in source order). Codegen
    /// emits one method per entry on the generated `Dagger<Sub>` class,
    /// matching the names the user's abstract class declared.
    pub child_entry_points: Vec<EntryPoint>,
    /// Where the parent's factory method appears in source.
    pub source: SourceSpan,
    /// Factory parameters declared on the parent's abstract method (M11).
    /// Empty for the M8 zero-arg shape. Each entry becomes a stored
    /// field on the child dagger and a [`Provider::FactoryParam`]
    /// binding inside `child_graph.bindings`.
    pub factory_params: Vec<FactoryParam>,
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
    /// Keys whose binding is **inherited** from the parent component (only
    /// non-empty for subcomponent graphs). Codegen routes these through
    /// `this.parent.<getX>()` instead of emitting a local factory.
    pub inherited_keys: HashSet<Key>,
    /// Subcomponent factories declared on this graph's owning component.
    /// Each entry corresponds to one parent entry-point method whose
    /// return type matches a [`SubcomponentDecl`].
    pub subcomponent_factories: Vec<SubcomponentFactory>,
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
#[allow(clippy::too_many_lines)]
pub fn build_and_validate(input: GraphInput<'_>) -> (DependencyGraph, Vec<Diagnostic>) {
    let (mut bindings, mut diagnostics) = aggregate_bindings(
        input.component.modules.as_slice(),
        input.modules,
        input.inject_classes,
        None,
    );

    // Index subcomponent class refs so we can recognize factory entry points.
    let mut sub_by_classref: HashMap<&ClassRef, &SubcomponentDecl> = HashMap::new();
    for sub in input.subcomponents {
        sub_by_classref.insert(&sub.class, sub);
    }

    // M11: prune `bindings` to only those reachable from non-subcomponent
    // entry points. The project-wide `@Inject` set includes classes that
    // only resolve inside a subcomponent (whose deps depend on factory
    // params); leaving them in the parent's binding map makes
    // `build_petgraph` emit spurious "missing binding" diagnostics for
    // their child-only deps, and would also make codegen emit dead
    // factory methods on the parent dagger.
    let parent_root_keys: Vec<Key> = input
        .component
        .entry_points
        .iter()
        .filter(|ep| {
            // Subcomponent factories don't contribute to parent
            // reachability — their child graph stands alone.
            if let Key::Class { module, name } = &ep.key {
                let cr = ClassRef {
                    module: module.clone(),
                    name: name.clone(),
                };
                !sub_by_classref.contains_key(&cr)
            } else {
                true
            }
        })
        .map(|ep| ep.key.clone())
        .collect();
    prune_unreachable_bindings(&mut bindings, &parent_root_keys);

    let (graph, node_for, missing) = build_petgraph(&bindings, None);
    diagnostics.extend(missing);

    let mut roots: Vec<Key> = Vec::with_capacity(input.component.entry_points.len());
    let mut subcomponent_factories: Vec<SubcomponentFactory> = Vec::new();
    for ep in &input.component.entry_points {
        // Is this entry point a subcomponent factory?
        let sub_match = if let Key::Class { module, name } = &ep.key {
            let cr = ClassRef {
                module: module.clone(),
                name: name.clone(),
            };
            sub_by_classref.get(&cr).copied()
        } else {
            None
        };
        if let Some(sub) = sub_match {
            // M11: a @Singleton subcomponent that takes runtime factory
            // parameters would cache the first call's `req` across every
            // subsequent invocation — almost always a bug. Reject it
            // loudly rather than producing surprising graphs.
            if !ep.factory_params.is_empty() && sub.scope == Scope::Singleton {
                diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::SingletonSubcomponentWithFactoryParams {
                        subcomponent: sub.class.clone(),
                    },
                    primary: Label {
                        span: sub.source.clone(),
                        message: "@Singleton @Subcomponent cannot take factory parameters"
                            .to_owned(),
                    },
                    related: vec![Label {
                        span: ep.source.clone(),
                        message: "factory declared here".to_owned(),
                    }],
                });
            }
            // M11: detect duplicate factory-param keys on the same
            // factory method. Two `Request` parameters would make any
            // child binding asking for `Request` ambiguous.
            for diag in detect_duplicate_factory_params(&ep.factory_params, &ep.source) {
                diagnostics.push(diag);
            }
            let (child_graph, child_diags) = build_child_graph(
                sub,
                input.modules,
                input.inject_classes,
                input.subcomponents,
                &bindings,
                &ep.factory_params,
            );
            diagnostics.extend(child_diags);
            subcomponent_factories.push(SubcomponentFactory {
                method_name: ep.name.clone(),
                subcomponent: sub.class.clone(),
                child_graph,
                child_entry_points: sub.entry_points.clone(),
                source: ep.source.clone(),
                factory_params: ep.factory_params.clone(),
            });
            continue;
        }

        // M11: factory parameters on a regular @Component entry point
        // would have nowhere to come from — `createApp()` is zero-arg.
        if !ep.factory_params.is_empty() {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::FactoryParamsOnNonSubcomponentEntry {
                    component: input.component.class.clone(),
                    method: ep.name.clone(),
                },
                primary: Label {
                    span: ep.source.clone(),
                    message: "@Component entry points must be zero-arg".to_owned(),
                },
                related: vec![],
            });
        }

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

    diagnostics.extend(detect_cycles(&graph, &bindings, &input.component.source));
    diagnostics.extend(detect_scope_mismatches(
        &bindings,
        input.component.scope,
        &input.component.source,
        "component is not @Singleton",
    ));

    let dep_graph = DependencyGraph {
        component: input.component.class.clone(),
        component_scope: input.component.scope,
        roots,
        bindings,
        graph,
        node_for,
        inherited_keys: HashSet::new(),
        subcomponent_factories,
    };
    (dep_graph, diagnostics)
}

/// Build a child subcomponent's graph using `parent_bindings` as a fallback
/// dictionary. Any dep satisfied by the parent is recorded in
/// [`DependencyGraph::inherited_keys`] and not added as a child node;
/// codegen will route those calls to `this.parent.<getX>()`.
///
/// `factory_params` (M11) become virtual `Provider::FactoryParam`
/// bindings injected into the child's binding map *before* the petgraph
/// pass, so any child binding that requests a factory-param key
/// resolves locally rather than via the parent. Factory params shadow
/// any same-keyed parent binding on purpose — that's the whole point.
fn build_child_graph(
    sub: &SubcomponentDecl,
    all_modules: &[ModuleDecl],
    inject_classes: &[Binding],
    subcomponents: &[SubcomponentDecl],
    parent_bindings: &HashMap<Key, Binding>,
    factory_params: &[FactoryParam],
) -> (DependencyGraph, Vec<Diagnostic>) {
    let (mut bindings, mut diagnostics) = aggregate_bindings(
        sub.modules.as_slice(),
        all_modules,
        inject_classes,
        Some(parent_bindings),
    );

    // M11: inject factory-param bindings. These take priority over both
    // module/inject bindings and parent-inherited bindings — the runtime
    // value supplied at the factory call site is authoritative.
    for fp in factory_params {
        bindings.insert(
            fp.key.clone(),
            Binding {
                key: fp.key.clone(),
                provider: Provider::FactoryParam {
                    name: fp.name.clone(),
                },
                scope: Scope::Unscoped,
                deps: vec![],
                source: fp.source.clone(),
                role: MultibindRole::None,
            },
        );
    }

    // M11: prune to bindings reachable from the subcomponent's entry
    // points. Same rationale as the parent path — keeps spurious
    // missing-dep diagnostics from leaking out of the project-wide
    // @Inject pool, and ensures codegen only emits factories that
    // matter to this component.
    let child_root_keys: Vec<Key> = sub.entry_points.iter().map(|ep| ep.key.clone()).collect();
    prune_unreachable_bindings_with_fallback(&mut bindings, &child_root_keys, parent_bindings);

    let (graph, node_for, missing) = build_petgraph(&bindings, Some(parent_bindings));
    diagnostics.extend(missing);

    // Compute which keys are inherited (reached via parent for some binding's deps).
    // Factory params satisfy locally — they are *never* inherited even
    // when the parent also has the same key.
    let mut inherited_keys: HashSet<Key> = HashSet::new();
    for b in bindings.values() {
        for dep in &b.deps {
            if !bindings.contains_key(dep) && parent_bindings.contains_key(dep) {
                inherited_keys.insert(dep.clone());
            }
        }
    }

    // Subcomponent factories on a subcomponent itself are out of scope for M8;
    // we just track entry points the same way we do on a component. A nested
    // factory entry would be reported as MissingBinding.
    let _ = subcomponents;

    let mut roots: Vec<Key> = Vec::with_capacity(sub.entry_points.len());
    for ep in &sub.entry_points {
        roots.push(ep.key.clone());
        if !node_for.contains_key(&ep.key) && !parent_bindings.contains_key(&ep.key) {
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
        // Entry points satisfied by parent become inherited too.
        if !node_for.contains_key(&ep.key) && parent_bindings.contains_key(&ep.key) {
            inherited_keys.insert(ep.key.clone());
        }
    }

    diagnostics.extend(detect_cycles(&graph, &bindings, &sub.source));
    diagnostics.extend(detect_scope_mismatches(
        &bindings,
        sub.scope,
        &sub.source,
        "subcomponent is not @Singleton",
    ));

    let child_graph = DependencyGraph {
        component: sub.class.clone(),
        component_scope: sub.scope,
        roots,
        bindings,
        graph,
        node_for,
        inherited_keys,
        subcomponent_factories: vec![],
    };
    (child_graph, diagnostics)
}

/// Collect bindings from a component-or-subcomponent's modules + project-wide
/// self-bindings, emitting a [`DiagnosticKind::Duplicate`] for any key
/// declared more than once.
///
/// When `parent_bindings` is `Some`, any binding whose key already exists in
/// the parent is **skipped** (inherited rather than redeclared).
fn aggregate_bindings(
    own_modules: &[ClassRef],
    all_modules: &[ModuleDecl],
    inject_classes: &[Binding],
    parent_bindings: Option<&HashMap<Key, Binding>>,
) -> (HashMap<Key, Binding>, Vec<Diagnostic>) {
    let mut bindings: HashMap<Key, Binding> = HashMap::new();
    let mut sources_for_key: HashMap<Key, Vec<SourceSpan>> = HashMap::new();
    // M9: pending @IntoSet contributions, keyed by element type. After the
    // raw-binding pass we synthesize one `Provider::SetMultibinding` per
    // entry under `Key::Set { element }`.
    let mut set_contribs: HashMap<Key, (Vec<SetContributor>, Scope, SourceSpan)> = HashMap::new();
    let component_modules: HashSet<&ClassRef> = own_modules.iter().collect();
    let inherits = |k: &Key| parent_bindings.is_some_and(|p| p.contains_key(k));

    for m in all_modules {
        if !component_modules.contains(&m.class) {
            continue;
        }
        for b in &m.provides {
            if matches!(b.role, MultibindRole::IntoSet) {
                collect_set_contributor(b, &mut set_contribs);
                continue;
            }
            if inherits(&b.key) {
                continue;
            }
            register_binding(b, &mut bindings, &mut sources_for_key);
        }
    }
    for b in inject_classes {
        // @Inject self-bindings can't carry @IntoSet in v0.1, but be
        // defensive and route them anyway so the IR shape stays consistent.
        if matches!(b.role, MultibindRole::IntoSet) {
            collect_set_contributor(b, &mut set_contribs);
            continue;
        }
        if inherits(&b.key) {
            continue;
        }
        register_binding(b, &mut bindings, &mut sources_for_key);
    }

    // Synthesize one Provider::SetMultibinding per element type. Multiple
    // contributions to the same Set<T> are intentional, not duplicates.
    for (set_key, (contributors, scope, source)) in set_contribs {
        if inherits(&set_key) {
            continue;
        }
        // Union of every contributor's deps; ordering doesn't matter for
        // graph semantics, but stable insertion order keeps codegen
        // deterministic.
        let mut deps: Vec<Key> = Vec::new();
        let mut seen: HashSet<Key> = HashSet::new();
        for c in &contributors {
            for d in &c.deps {
                if seen.insert(d.clone()) {
                    deps.push(d.clone());
                }
            }
        }
        let synth = Binding {
            key: set_key.clone(),
            provider: Provider::SetMultibinding { contributors },
            scope,
            deps,
            source: source.clone(),
            role: MultibindRole::None,
        };
        bindings.insert(set_key, synth);
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

/// Append a contributor record to the pending multibinding map under the
/// `Key::Set { element: <binding.key> }` aggregate. Validates the binding's
/// provider shape — only `Provider::ProvidesMethod` is supported in v0.1
/// (the parser already rejected `@IntoSet` on `@Binds` / `@Inject`).
fn collect_set_contributor(
    b: &Binding,
    set_contribs: &mut HashMap<Key, (Vec<SetContributor>, Scope, SourceSpan)>,
) {
    let Provider::ProvidesMethod { module, method } = &b.provider else {
        // The parser is responsible for ensuring this; skip silently if it
        // ever gets through. The graph would surface the binding as a
        // missing key for whoever requested Set<T>, which is acceptable
        // best-effort behavior.
        return;
    };
    let element_key = b.key.clone();
    let set_key = Key::Set {
        element: Box::new(element_key),
    };
    let entry = set_contribs
        .entry(set_key)
        .or_insert_with(|| (Vec::new(), b.scope, b.source.clone()));
    entry.0.push(SetContributor {
        module: module.clone(),
        method: method.clone(),
        deps: b.deps.clone(),
        source: b.source.clone(),
    });
}

/// Build the petgraph DAG over `bindings` and return any missing-dep
/// diagnostics encountered while wiring edges. If `parent_bindings` is
/// supplied, deps satisfied by the parent are silently dropped (they
/// become inherited at codegen time).
fn build_petgraph(
    bindings: &HashMap<Key, Binding>,
    parent_bindings: Option<&HashMap<Key, Binding>>,
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
            if let Some(&to) = node_for.get(dep) {
                graph.add_edge(from, to, ());
            } else if !parent_bindings.is_some_and(|p| p.contains_key(dep)) {
                missing.push(missing_for_dep(dep.clone(), key.clone(), binding));
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
    fallback_span: &SourceSpan,
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
            .map_or_else(|| fallback_span.clone(), |b| b.source.clone());
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
/// inside a non-`Singleton` component-or-subcomponent.
fn detect_scope_mismatches(
    bindings: &HashMap<Key, Binding>,
    owner_scope: Scope,
    owner_source: &SourceSpan,
    owner_kind: &str,
) -> Vec<Diagnostic> {
    if owner_scope == Scope::Singleton {
        return Vec::new();
    }
    bindings
        .values()
        .filter(|b| b.scope == Scope::Singleton)
        .map(|b| Diagnostic {
            kind: DiagnosticKind::ScopeMismatch {
                key: b.key.clone(),
                binding_scope: b.scope,
                component_scope: owner_scope,
            },
            primary: Label {
                span: b.source.clone(),
                message: "singleton binding".to_owned(),
            },
            related: vec![Label {
                span: owner_source.clone(),
                message: owner_kind.to_owned(),
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

/// Prune `bindings` to entries reachable from `roots` via the binding
/// dep edges. Bindings whose keys aren't transitively requested by any
/// entry point are dropped — this matches Dagger's "only validate what's
/// used" policy and avoids surfacing spurious missing-dep diagnostics
/// for project-wide `@Inject` classes that only make sense in a
/// subcomponent's scope (M11).
fn prune_unreachable_bindings(bindings: &mut HashMap<Key, Binding>, roots: &[Key]) {
    let mut reached: HashSet<Key> = HashSet::new();
    let mut queue: Vec<Key> = roots.to_vec();
    while let Some(k) = queue.pop() {
        if !reached.insert(k.clone()) {
            continue;
        }
        if let Some(b) = bindings.get(&k) {
            for d in &b.deps {
                if !reached.contains(d) {
                    queue.push(d.clone());
                }
            }
        }
    }
    bindings.retain(|k, _| reached.contains(k));
}

/// Same as [`prune_unreachable_bindings`] but accepts a `parent_bindings`
/// fallback used to follow inherited deps across the parent boundary
/// (so a child binding's parent-satisfied dep still pulls in any further
/// child deps reachable from it). Today the inherited side is shallow —
/// we only need to walk into the child's own bindings — so the
/// implementation collapses back to a regular pure-child walk; the
/// `parent_bindings` argument is kept for symmetry and future
/// generalization (e.g. nested subcomponents).
fn prune_unreachable_bindings_with_fallback(
    bindings: &mut HashMap<Key, Binding>,
    roots: &[Key],
    parent_bindings: &HashMap<Key, Binding>,
) {
    let _ = parent_bindings;
    prune_unreachable_bindings(bindings, roots);
}

/// Report `DuplicateFactoryParam` if the same key appears more than
/// once in a single factory's parameter list (M11). Two `Request` params
/// would make `req: Request` ambiguous for any child binding asking for
/// `Request`.
fn detect_duplicate_factory_params(
    params: &[FactoryParam],
    fallback_span: &SourceSpan,
) -> Vec<Diagnostic> {
    let mut by_key: HashMap<Key, Vec<&FactoryParam>> = HashMap::new();
    for p in params {
        by_key.entry(p.key.clone()).or_default().push(p);
    }
    let mut out = Vec::new();
    for (key, sites) in by_key {
        if sites.len() < 2 {
            continue;
        }
        let mut iter = sites.iter();
        let first = iter.next().expect("len >= 2");
        let related: Vec<Label> = iter
            .map(|p| Label {
                span: p.source.clone(),
                message: format!("also bound here as `{}`", p.name),
            })
            .collect();
        out.push(Diagnostic {
            kind: DiagnosticKind::DuplicateFactoryParam { key },
            primary: Label {
                span: first.source.clone(),
                message: format!("first declared as `{}`", first.name),
            },
            related,
        });
        let _ = fallback_span;
    }
    out
}

fn missing_for_dep(missing: Key, requested_by: Key, binding: &Binding) -> Diagnostic {
    let provider_kind = match &binding.provider {
        Provider::InjectCtor { .. } => "@Inject ctor",
        Provider::ProvidesMethod { .. } => "@Provides method",
        Provider::Binds { .. } => "@Binds method",
        Provider::SetMultibinding { .. } => "@IntoSet aggregate",
        Provider::FactoryParam { .. } => "subcomponent factory parameter",
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
        Binding, ClassRef, ComponentDecl, EntryPoint, FactoryParam, Key, ModuleDecl, ModulePath,
        MultibindRole, Provider, Scope, SourceSpan,
    };

    fn key(name: &str) -> Key {
        Key::Class {
            module: ModulePath::from_abs(format!("/p/{name}.ts")),
            name: name.to_owned(),
        }
    }

    fn class_ref(name: &str) -> ClassRef {
        ClassRef {
            module: ModulePath::from_abs(format!("/p/{name}.ts")),
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
            role: MultibindRole::None,
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
            role: MultibindRole::None,
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
            factory_params: vec![],
        }
    }

    #[test]
    fn empty_component_with_no_entry_points_is_valid() {
        let comp = empty_component("Comp", vec![], Scope::Unscoped);
        let (g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[],
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
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
            subcomponents: &[],
        });
        assert!(ds.is_empty(), "{ds:?}");
    }

    fn ep_with_factory_params(name: &str, k: Key, params: Vec<FactoryParam>) -> EntryPoint {
        EntryPoint {
            name: name.to_owned(),
            key: k,
            source: span("Component", 50),
            factory_params: params,
        }
    }

    fn factory_param(name: &str, k: Key) -> FactoryParam {
        FactoryParam {
            name: name.to_owned(),
            key: k,
            source: span(name, 0),
        }
    }

    #[test]
    fn factory_params_become_virtual_bindings_in_child_graph() {
        // Parent has a `requestComponent(req: Request, res: Response)`
        // factory whose subcomponent has a @Provides that consumes
        // both. The graph layer should inject Provider::FactoryParam
        // bindings so the @Provides resolves locally.
        let request_key = key("Request");
        let response_key = key("Response");
        let handler_key = key("Handler");

        let request_module = ModuleDecl {
            class: class_ref("RequestModule"),
            provides: vec![Binding {
                key: handler_key.clone(),
                provider: Provider::ProvidesMethod {
                    module: class_ref("RequestModule"),
                    method: "handler".into(),
                },
                scope: Scope::Unscoped,
                deps: vec![request_key.clone(), response_key.clone()],
                source: span("RequestModule", 100),
                role: MultibindRole::None,
            }],
            source: span("RequestModule", 0),
        };

        let mut sub = empty_component(
            "RequestComponent",
            vec![class_ref("RequestModule")],
            Scope::Unscoped,
        );
        sub.entry_points.push(ep("handler", handler_key.clone()));
        let sub = SubcomponentDecl {
            class: sub.class,
            modules: sub.modules,
            scope: sub.scope,
            entry_points: sub.entry_points,
            source: sub.source,
        };

        let mut parent = empty_component("App", vec![], Scope::Unscoped);
        parent.entry_points.push(ep_with_factory_params(
            "requestComponent",
            Key::Class {
                module: ModulePath::from_abs("/p/RequestComponent.ts"),
                name: "RequestComponent".into(),
            },
            vec![
                factory_param("req", request_key.clone()),
                factory_param("res", response_key.clone()),
            ],
        ));

        let (g, ds) = build_and_validate(GraphInput {
            component: &parent,
            modules: &[request_module],
            inject_classes: &[],
            subcomponents: &[sub],
        });
        assert!(ds.is_empty(), "expected no diagnostics, got {ds:?}");
        assert_eq!(g.subcomponent_factories.len(), 1);
        let fact = &g.subcomponent_factories[0];
        assert_eq!(fact.factory_params.len(), 2);
        // The child graph should have FactoryParam bindings for both keys.
        assert!(matches!(
            fact.child_graph.bindings.get(&request_key).map(|b| &b.provider),
            Some(Provider::FactoryParam { name }) if name == "req"
        ));
        assert!(matches!(
            fact.child_graph.bindings.get(&response_key).map(|b| &b.provider),
            Some(Provider::FactoryParam { name }) if name == "res"
        ));
    }

    #[test]
    fn factory_params_on_regular_component_entry_is_rejected() {
        let mut comp = empty_component("App", vec![], Scope::Unscoped);
        comp.entry_points.push(ep_with_factory_params(
            "stuff",
            key("Heater"),
            vec![factory_param("ctx", key("Ctx"))],
        ));
        // Provide a Heater binding so the only diagnostic is the factory-param one.
        let heater = inject("Heater", vec![], Scope::Unscoped);
        let (_g, ds) = build_and_validate(GraphInput {
            component: &comp,
            modules: &[],
            inject_classes: &[heater],
            subcomponents: &[],
        });
        assert!(
            ds.iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::FactoryParamsOnNonSubcomponentEntry { .. }
            )),
            "expected FactoryParamsOnNonSubcomponentEntry, got {ds:?}",
        );
    }

    #[test]
    fn duplicate_factory_param_keys_are_rejected() {
        let request_key = key("Request");
        let mut sub = empty_component("Req", vec![], Scope::Unscoped);
        sub.entry_points.push(ep("noop", key("Noop")));
        let sub = SubcomponentDecl {
            class: sub.class,
            modules: sub.modules,
            scope: sub.scope,
            entry_points: vec![],
            source: sub.source,
        };
        let mut parent = empty_component("App", vec![], Scope::Unscoped);
        parent.entry_points.push(ep_with_factory_params(
            "req",
            Key::Class {
                module: ModulePath::from_abs("/p/Req.ts"),
                name: "Req".into(),
            },
            vec![
                factory_param("a", request_key.clone()),
                factory_param("b", request_key),
            ],
        ));
        let (_g, ds) = build_and_validate(GraphInput {
            component: &parent,
            modules: &[],
            inject_classes: &[],
            subcomponents: &[sub],
        });
        assert!(
            ds.iter()
                .any(|d| matches!(d.kind, DiagnosticKind::DuplicateFactoryParam { .. })),
            "expected DuplicateFactoryParam, got {ds:?}",
        );
    }

    #[test]
    fn singleton_subcomponent_with_factory_params_is_rejected() {
        let mut sub = empty_component("Req", vec![], Scope::Singleton);
        sub.entry_points.push(ep("noop", key("Noop")));
        let sub = SubcomponentDecl {
            class: sub.class,
            modules: sub.modules,
            scope: sub.scope,
            entry_points: vec![],
            source: sub.source,
        };
        let mut parent = empty_component("App", vec![], Scope::Singleton);
        parent.entry_points.push(ep_with_factory_params(
            "req",
            Key::Class {
                module: ModulePath::from_abs("/p/Req.ts"),
                name: "Req".into(),
            },
            vec![factory_param("ctx", key("Ctx"))],
        ));
        let (_g, ds) = build_and_validate(GraphInput {
            component: &parent,
            modules: &[],
            inject_classes: &[],
            subcomponents: &[sub],
        });
        assert!(
            ds.iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::SingletonSubcomponentWithFactoryParams { .. }
            )),
            "expected SingletonSubcomponentWithFactoryParams, got {ds:?}",
        );
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
            subcomponents: &[],
        });
        assert!(ds.is_empty());
        assert_eq!(g.node_count(), 0);
    }
}
