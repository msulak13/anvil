//! Emit one `*.anvil.ts` file for a single `@Component`.
//!
//! Pipeline:
//!
//! 1. Run [`anvil_core::graph::build_and_validate`] over the component +
//!    project bindings. Any diagnostic short-circuits to
//!    [`EmitError::Invalid`].
//! 2. Build a topologically-ordered list of bindings (deps first;
//!    lexicographic tie-break on `Key`) so two equivalent inputs yield
//!    byte-identical outputs.
//! 3. Construct the generated TS as a Rust [`String`]: imports + a
//!    `Dagger<Component>` class with one factory method per binding +
//!    entry-point methods + a `create<Component>` helper.
//! 4. Hand that string to [`oxc_parser`] to verify it's structurally
//!    valid TypeScript, then [`oxc_codegen`] to canonicalize formatting.
//! 5. Prepend the banner + source comment.
//!
//! M6 supports both `Scope::Unscoped` (fresh instance per call) and
//! `Scope::Singleton` (lazily cached on a private field via `??=`). The
//! cache field's name is `_<lower-camel name>` and its type is
//! `<Type> | undefined`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_span::SourceType;

use anvil_core::graph::{build_and_validate, DependencyGraph, GraphInput, SubcomponentFactory};
use anvil_core::ir::{
    Binding, ClassRef, ComponentDecl, EntryPoint, Key, ModuleDecl, Provider, Scope,
    SubcomponentDecl,
};

use crate::{banner_for, EmitError, Result};

/// Emit the contents of `<component>.anvil.ts` for one component.
///
/// `version` is rendered into the banner so users can correlate generated
/// files with the toolchain that produced them.
///
/// # Errors
///
/// - [`EmitError::Invalid`] if validation produces any diagnostics.
/// - [`EmitError::BadComponentPath`] if the component's `module` field has
///   no parent directory (shouldn't happen post-M2, but guarded for safety).
/// - [`EmitError::EmittedSyntaxError`] if the string-built TS fails to
///   parse — that's an emitter bug; the error carries the parser's
///   complaints to make it easy to localize.
pub fn emit_component(
    component: &ComponentDecl,
    modules: &[ModuleDecl],
    inject_classes: &[Binding],
    subcomponents: &[SubcomponentDecl],
    version: &str,
) -> Result<String> {
    let (graph, diagnostics) = build_and_validate(GraphInput {
        component,
        modules,
        inject_classes,
        subcomponents,
    });
    if !diagnostics.is_empty() {
        return Err(EmitError::Invalid(diagnostics));
    }

    let component_path = PathBuf::from(&component.class.module.abs);
    let out_dir = component_path
        .parent()
        .ok_or_else(|| EmitError::BadComponentPath(component_path.display().to_string()))?
        .to_path_buf();

    let parent_topo = topo_order(&graph);
    let mut imports = Imports::default();
    // The component class is used in `extends ComponentName` — a value position.
    imports.add_value(&out_dir, &component.class);
    populate_imports(&mut imports, &out_dir, &graph, &parent_topo);
    // Imports for each child subcomponent's bindings + the subcomponent class itself.
    let mut child_topos: Vec<(usize, Vec<Key>)> =
        Vec::with_capacity(graph.subcomponent_factories.len());
    for (i, fact) in graph.subcomponent_factories.iter().enumerate() {
        // Subcomponent class is used in `extends SubName` — a value position.
        imports.add_value(&out_dir, &fact.subcomponent);
        let child_topo = topo_order(&fact.child_graph);
        populate_imports(&mut imports, &out_dir, &fact.child_graph, &child_topo);
        child_topos.push((i, child_topo));
    }

    let body = build_ts_source(component, &graph, &parent_topo, &child_topos, &imports);

    // Validate that we emitted parseable TS, then re-print to canonicalize.
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &body, SourceType::ts()).parse();
    if !ret.errors.is_empty() {
        return Err(EmitError::EmittedSyntaxError {
            errors: ret.errors.iter().map(|e| format!("{e:?}")).collect(),
        });
    }
    let canonical = Codegen::new().build(&ret.program).code;

    let source_relative =
        relative_ts_specifier(&out_dir, &component_path, /* keep_ext */ true);
    let mut output = String::new();
    output.push_str(&banner_for(version));
    writeln!(output, "// Source: {source_relative}").expect("write to String");
    output.push_str(&canonical);
    Ok(output)
}

fn key_module(key: &Key) -> &str {
    match key {
        Key::Class { module, .. } => &module.abs,
        Key::Token { .. } => "<token>",
        Key::Set { element } => key_module(element),
    }
}

/// TypeScript type-string for a [`Key`]: `"Heater"` for [`Key::Class`] and
/// `"Set<Heater>"` for [`Key::Set`]. For `Key::Token`, returns `"unknown"` —
/// callers that need the actual inner type should use `type_string_for_binding`.
fn type_string_of(key: &Key) -> String {
    match key {
        Key::Class {
            name, type_args, ..
        } if type_args.is_empty() => name.clone(),
        Key::Class {
            name, type_args, ..
        } => {
            let args: Vec<String> = type_args.iter().map(type_string_of).collect();
            format!("{}<{}>", name, args.join(", "))
        }
        Key::Token { .. } => "unknown".to_owned(),
        Key::Set { element } => format!("Set<{}>", type_string_of(element)),
    }
}

/// TypeScript type-string for a binding. For `Key::Token` bindings, uses
/// `token_inner_type` if available; for all others delegates to `type_string_of`.
fn type_string_for_binding(b: &Binding) -> String {
    match &b.key {
        Key::Token { .. } => b
            .token_inner_type
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        key => type_string_of(key),
    }
}

/// `Heater` → `getHeater`. `Set<Plugin>` → `getSetOfPlugin`.
/// `Token("primary-db")` → `getPrimaryDb`.
fn factory_name_for(key: &Key) -> String {
    match key {
        Key::Class {
            name, type_args, ..
        } if type_args.is_empty() => format!("get{name}"),
        Key::Class {
            name, type_args, ..
        } => {
            format!("get{}Of{}", name, type_args_label(type_args))
        }
        Key::Token { name } => format!("get{}", token_pascal_case(name)),
        Key::Set { element } => format!("getSetOf{}", element_label(element)),
    }
}

/// `Heater` → `_heater`. `Set<Plugin>` → `_setOfPlugin`.
fn cache_field_for(key: &Key) -> String {
    match key {
        Key::Class {
            name, type_args, ..
        } if type_args.is_empty() => cache_field_name(name),
        Key::Class {
            name, type_args, ..
        } => {
            format!("_{}Of{}", lower_first(name), type_args_label(type_args))
        }
        Key::Token { name } => format!("_{}", lower_first(&token_pascal_case(name))),
        Key::Set { element } => format!("_setOf{}", element_label(element)),
    }
}

/// Camel-cased label used inside `Set<T>`-derived and `Repository<User>`-derived identifiers.
/// `Plugin` → `Plugin`; `Set<Plugin>` (nested) → `SetOfPlugin`;
/// `Token("primary-db")` → `PrimaryDb`.
fn element_label(key: &Key) -> String {
    match key {
        Key::Class {
            name, type_args, ..
        } if type_args.is_empty() => name.clone(),
        Key::Class {
            name, type_args, ..
        } => {
            format!("{}Of{}", name, type_args_label(type_args))
        }
        Key::Token { name } => token_pascal_case(name),
        Key::Set { element } => format!("SetOf{}", element_label(element)),
    }
}

/// Convert generic type args to a label suffix: `[User]` → `User`,
/// `[User, Role]` → `UserAndRole`.
fn type_args_label(args: &[Key]) -> String {
    args.iter()
        .map(element_label)
        .collect::<Vec<_>>()
        .join("And")
}

/// Convert a token name like `"primary-db"` to `PascalCase` `"PrimaryDb"`.
fn token_pascal_case(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|f| {
                    let upper: String = f.to_uppercase().collect();
                    upper + chars.as_str()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Lowercase first character of a string.
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let lower: String = first.to_lowercase().collect();
            lower + chars.as_str()
        }
        None => String::new(),
    }
}

/// Stable comparison key for ties: `"Name@module"`.
fn lex_key(k: &Key) -> String {
    match k {
        Key::Token { name } => format!("Token({name})@<token>"),
        _ => format!("{}@{}", type_string_of(k), key_module(k)),
    }
}

/// Deps-first topological order, with lexicographic tie-break, computed by
/// DFS post-order. The graph is acyclic by the time we get here (M3
/// validation rejected cycles), so the recursion terminates.
fn topo_order(graph: &DependencyGraph) -> Vec<Key> {
    let mut visited: HashSet<Key> = HashSet::new();
    let mut out: Vec<Key> = Vec::new();
    let mut roots: Vec<&Key> = graph.bindings.keys().collect();
    roots.sort_by_key(|k| lex_key(k));
    for k in roots {
        dfs(k, graph, &mut visited, &mut out);
    }
    out
}

fn dfs(k: &Key, graph: &DependencyGraph, visited: &mut HashSet<Key>, out: &mut Vec<Key>) {
    if !visited.insert(k.clone()) {
        return;
    }
    if let Some(b) = graph.bindings.get(k) {
        let mut deps: Vec<&Key> = b.deps.iter().collect();
        deps.sort_by_key(|d| lex_key(d));
        for d in deps {
            dfs(d, graph, visited, out);
        }
        // Only emit topo entries for keys that *have* a local binding.
        // Inherited / parent-satisfied / factory-param-on-parent deps
        // are visited so their transitive deps are queued, but they
        // don't produce a factory in this graph and shouldn't be
        // touched by `populate_imports` or `emit_class_body`.
        out.push(k.clone());
    }
}

/// Tracks which imported names are used as runtime values vs. type annotations only.
///
/// Symbols used as values (e.g. `new X()`, `X.method()`, `extends X`) need a
/// regular `import { X }`. Symbols used only in type positions (return types,
/// field types, generic args) must use `import type { X }` when the consumer's
/// `tsconfig.json` enables `verbatimModuleSyntax`.
#[derive(Default)]
struct Imports {
    /// Names referenced at runtime (constructors, static method calls, extends).
    value: BTreeMap<String, BTreeSet<String>>,
    /// Names referenced only in type positions (annotations, generic args).
    type_only: BTreeMap<String, BTreeSet<String>>,
}

impl Imports {
    fn add_value(&mut self, out_dir: &Path, cref: &ClassRef) {
        let spec = import_specifier_for(out_dir, &cref.module);
        self.value
            .entry(spec)
            .or_default()
            .insert(cref.name.clone());
    }

    fn add_type(&mut self, out_dir: &Path, cref: &ClassRef) {
        let spec = import_specifier_for(out_dir, &cref.module);
        self.type_only
            .entry(spec)
            .or_default()
            .insert(cref.name.clone());
    }
}

/// Walk every binding in `topo` and add its produced type plus its
/// provider's referenced class refs to `imports`. Used for both the
/// parent component and each child subcomponent.
fn populate_imports(imports: &mut Imports, out_dir: &Path, graph: &DependencyGraph, topo: &[Key]) {
    for k in topo {
        let b = &graph.bindings[k];
        // The Set<T> type itself is a built-in — no import needed for the
        // outer key. The element class is reachable via the contributors'
        // module imports below (and as a Key::Class binding elsewhere if
        // any). For Key::Class we import the bound class name for its type
        // annotation (return type, cache field). Key::Token has no class to
        // import — the inner type is handled via token_inner_type and its deps.
        match &b.key {
            Key::Class {
                module,
                name,
                type_args,
            } => {
                // The bound class appears in type positions (return type,
                // cache-field type). It's a *value* import only when the
                // provider constructs it directly via InjectCtor (handled
                // below); for ProvidesMethod the class name is type-only.
                imports.add_type(
                    out_dir,
                    &ClassRef {
                        module: module.clone(),
                        name: name.clone(),
                    },
                );
                import_key_args(imports, out_dir, type_args);
            }
            Key::Set { element } => {
                // Make sure the element type is importable so the
                // `Set<Element>` type annotation type-checks even when no
                // Class binding for the element exists in this graph.
                if let Key::Class { module, name, .. } = element.as_ref() {
                    imports.add_type(
                        out_dir,
                        &ClassRef {
                            module: module.clone(),
                            name: name.clone(),
                        },
                    );
                }
            }
            Key::Token { .. } => {
                // Token bindings have no outer class to import.
                // The inner type is surfaced through the TS cast; its import
                // comes via the binding's deps (the actual implementation class).
            }
        }
        match &b.provider {
            Provider::InjectCtor { class } => {
                // `new ClassName(deps)` — the class is used as a value.
                imports.add_value(out_dir, class);
            }
            Provider::ProvidesMethod { module, .. } => {
                // `ModuleName.method(deps)` — the module class is used as a value.
                imports.add_value(out_dir, module);
            }
            // `Binds` and `FactoryParam` need no extra import:
            // - `Binds`'s target class arrives via its own `deps` entry
            //   when the topo walker visits that binding. The owning
            //   @Module class is abstract, so no static reference exists.
            // - `FactoryParam`'s type class is already imported when
            //   the binding's outer key was processed above. The
            //   binding's value comes from a stored ctor field.
            Provider::Binds { .. } | Provider::FactoryParam { .. } => {}
            Provider::SetMultibinding { contributors } => {
                for c in contributors {
                    // `ContribModule.method(deps)` — value import.
                    imports.add_value(out_dir, &c.module);
                }
            }
        }
    }
}

/// Recursively add type-only import entries for generic type arguments.
fn import_key_args(imports: &mut Imports, out_dir: &Path, args: &[Key]) {
    for arg in args {
        match arg {
            Key::Class {
                module,
                name,
                type_args,
            } => {
                imports.add_type(
                    out_dir,
                    &ClassRef {
                        module: module.clone(),
                        name: name.clone(),
                    },
                );
                import_key_args(imports, out_dir, type_args);
            }
            Key::Token { .. } | Key::Set { .. } => {}
        }
    }
}

/// Decide what string to emit as the import specifier for `mp`.
///
/// **Rule:** if `mp` resolves into `node_modules`, prefer the user's
/// original specifier (`"express"`, `"@scope/pkg"`, …). A relative path
/// into `node_modules/foo/index.d.ts` would be brittle and wrong — the
/// dagger consumes the package by name like every other importer.
///
/// For project-internal paths (or when `original` is unavailable),
/// recompute a relative path from the dagger's output directory. This
/// is correct even when the original specifier was relative to *a
/// different* importing file (e.g. the @Component imports `./pump` but
/// a deep `@Module` imports `../pump` — both must become `./pump`-shaped
/// when emitted alongside the dagger).
fn import_specifier_for(out_dir: &Path, mp: &anvil_core::ir::ModulePath) -> String {
    if mp.is_node_modules() {
        if let Some(orig) = mp.original.as_deref() {
            return orig.to_owned();
        }
    }
    let abs = PathBuf::from(&mp.abs);
    relative_ts_specifier(out_dir, &abs, /* keep_ext */ false)
}

#[allow(clippy::too_many_lines)]
fn build_ts_source(
    component: &ComponentDecl,
    graph: &DependencyGraph,
    bindings_topo: &[Key],
    child_topos: &[(usize, Vec<Key>)],
    imports: &Imports,
) -> String {
    let mut s = String::new();

    // Collect all specifiers from both maps, keeping BTreeMap sort order.
    let mut all_specs: BTreeSet<&str> = BTreeSet::new();
    all_specs.extend(imports.value.keys().map(String::as_str));
    all_specs.extend(imports.type_only.keys().map(String::as_str));

    for spec in &all_specs {
        let value_names = imports.value.get(*spec);
        let type_names = imports.type_only.get(*spec);
        match (value_names, type_names) {
            (Some(vals), None) => {
                let names: Vec<&str> = vals.iter().map(String::as_str).collect();
                writeln!(s, "import {{ {} }} from \"{spec}\";", names.join(", "))
                    .expect("write to String");
            }
            (None, Some(types)) => {
                let names: Vec<&str> = types.iter().map(String::as_str).collect();
                writeln!(s, "import type {{ {} }} from \"{spec}\";", names.join(", "))
                    .expect("write to String");
            }
            (Some(vals), Some(types)) => {
                // Mixed specifier: emit value names normally; emit type-only
                // names with an inline `type` modifier so `verbatimModuleSyntax`
                // is satisfied without splitting into two import statements.
                let mut names: Vec<String> = vals.iter().cloned().collect();
                for t in types {
                    if !vals.contains(t) {
                        names.push(format!("type {t}"));
                    }
                }
                names.sort();
                writeln!(s, "import {{ {} }} from \"{spec}\";", names.join(", "))
                    .expect("write to String");
            }
            (None, None) => {}
        }
    }
    s.push('\n');

    let comp_name = &component.class.name;
    let dagger_name = format!("Anvil{comp_name}");

    // Set of entry-point method names that are subcomponent factories;
    // these need a different emission shape than regular entry points.
    let sub_factory_method_names: HashSet<&str> = graph
        .subcomponent_factories
        .iter()
        .map(|f| f.method_name.as_str())
        .collect();
    // Union of every key any child subcomponent inherits from this
    // parent. Parent factories for these keys are emitted as non-private
    // so `this.parent.<getX>()` from the child class typechecks.
    let mut parent_keys_exposed: HashSet<Key> = HashSet::new();
    for fact in &graph.subcomponent_factories {
        for k in &fact.child_graph.inherited_keys {
            parent_keys_exposed.insert(k.clone());
        }
    }

    // M12: an "async-resolving" graph has at least one async @Provides
    // binding. The dagger emits a `_resolve` phase that awaits each in
    // topo order; `static create()` becomes async and the top-level
    // `createX()` returns `Promise<X>`.
    let parent_is_async = graph.is_async();

    writeln!(s, "export class {dagger_name} extends {comp_name} {{").expect("write to String");
    emit_class_body(
        &mut s,
        graph,
        bindings_topo,
        /* parent_dagger */ None,
        /* subcomponent_factories */ &graph.subcomponent_factories,
        /* sub_factory_method_names */ &sub_factory_method_names,
        /* entry_points */ &component.entry_points,
        /* expose_factories_for */ &parent_keys_exposed,
        /* graph_is_async */ parent_is_async,
        /* self_dagger */ &dagger_name,
    );
    if parent_is_async {
        writeln!(
            s,
            "  static async create(): Promise<{comp_name}> {{ const d = new {dagger_name}(); await {dagger_name}._resolve(d); return d; }}"
        )
        .expect("write to String");
    } else {
        writeln!(
            s,
            "  static create(): {comp_name} {{ return new {dagger_name}(); }}"
        )
        .expect("write to String");
    }
    s.push_str("}\n");
    if parent_is_async {
        writeln!(
            s,
            "export async function create{comp_name}(): Promise<{comp_name}> {{ return {dagger_name}.create(); }}"
        )
        .expect("write to String");
    } else {
        writeln!(
            s,
            "export function create{comp_name}(): {comp_name} {{ return {dagger_name}.create(); }}"
        )
        .expect("write to String");
    }

    // One Dagger<Sub> class per subcomponent factory.
    for (factory_idx, child_topo) in child_topos {
        let fact = &graph.subcomponent_factories[*factory_idx];
        let sub_name = &fact.subcomponent.name;
        let sub_dagger = format!("Anvil{sub_name}");
        let child_is_async = fact.child_graph.is_async();
        s.push('\n');
        writeln!(s, "export class {sub_dagger} extends {sub_name} {{").expect("write to String");
        // M11: ctor takes the parent dagger plus one private field per
        // factory parameter the parent threaded in. `private` on a
        // ctor param both stores it as a field *and* keeps it
        // encapsulated — the only callers are this class's own
        // factory methods.
        let mut ctor_params = format!("private parent: {dagger_name}");
        for fp in &fact.factory_params {
            ctor_params.push_str(", private ");
            ctor_params.push_str(&fp.name);
            ctor_params.push_str(": ");
            ctor_params.push_str(&type_string_of(&fp.key));
        }
        writeln!(s, "  constructor({ctor_params}) {{ super(); }}").expect("write to String");
        emit_class_body(
            &mut s,
            &fact.child_graph,
            child_topo,
            /* parent_dagger */ Some(&dagger_name),
            /* subcomponent_factories */ &[],
            /* sub_factory_method_names */ &HashSet::new(),
            /* entry_points */ &fact.child_entry_points,
            /* expose_factories_for */ &HashSet::new(),
            /* graph_is_async */ child_is_async,
            /* self_dagger */ &sub_dagger,
        );
        if child_is_async {
            // Async `static create` that takes the same args as the
            // ctor — the parent's factory method forwards `req`/`res`
            // straight through. We emit `create` rather than awaiting
            // inside the parent's factory body so the child class is
            // self-sufficient (e.g. unit tests can call it directly).
            let mut create_params = format!("parent: {dagger_name}");
            for fp in &fact.factory_params {
                create_params.push_str(", ");
                create_params.push_str(&fp.name);
                create_params.push_str(": ");
                create_params.push_str(&type_string_of(&fp.key));
            }
            let mut forwarded = String::from("parent");
            for fp in &fact.factory_params {
                forwarded.push_str(", ");
                forwarded.push_str(&fp.name);
            }
            writeln!(
                s,
                "  static async create({create_params}): Promise<{sub_name}> {{ const d = new {sub_dagger}({forwarded}); await {sub_dagger}._resolve(d); return d; }}"
            )
            .expect("write to String");
        }
        s.push_str("}\n");
    }

    s
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_class_body(
    s: &mut String,
    graph: &DependencyGraph,
    topo: &[Key],
    parent_dagger: Option<&str>,
    subcomponent_factories: &[SubcomponentFactory],
    sub_factory_method_names: &HashSet<&str>,
    entry_points: &[EntryPoint],
    expose_factories_for: &HashSet<Key>,
    graph_is_async: bool,
    self_dagger: &str,
) {
    // Cache fields for every singleton binding, in topo order so they
    // match the factory order users will read.
    for k in topo {
        let b = &graph.bindings[k];
        if matches!(b.scope, Scope::Singleton) {
            let type_string = type_string_for_binding(b);
            let field = cache_field_for(k);
            writeln!(s, "  private {field}: {type_string} | undefined;").expect("write to String");
        }
    }

    // Helper that builds the bare body expression for a binding (no
    // surrounding `return …;` and no caching). Used both for sync
    // factory bodies and for the async `_resolve` phase below.
    let body_expr_for = |k: &Key| -> String {
        let b = &graph.bindings[k];
        let dep_args = b
            .deps
            .iter()
            .map(|d| dep_call(d, graph, parent_dagger))
            .collect::<Vec<_>>()
            .join(", ");
        let raw_expr = match &b.provider {
            Provider::InjectCtor { class } => format!("new {}({})", class.name, dep_args),
            Provider::ProvidesMethod { module, method, .. } => {
                format!("{}.{}({})", module.name, method, dep_args)
            }
            Provider::Binds { target } => dep_call(target, graph, parent_dagger),
            Provider::SetMultibinding { contributors } => {
                let parts: Vec<String> = contributors
                    .iter()
                    .map(|c| {
                        let args = c
                            .deps
                            .iter()
                            .map(|d| dep_call(d, graph, parent_dagger))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}.{}({})", c.module.name, c.method, args)
                    })
                    .collect();
                format!("new Set([{}])", parts.join(", "))
            }
            Provider::FactoryParam { name } => format!("this.{name}"),
        };
        // M14: Token bindings need a cast so the generated code is type-safe.
        // The @Provides method returns the raw value (e.g. a string or Database
        // instance); the dagger's getter returns the inner type T.
        if let Key::Token { .. } = k {
            if let Some(inner_type) = &b.token_inner_type {
                return format!("{raw_expr} as unknown as {inner_type}");
            }
        }
        raw_expr
    };

    // M12: in async-resolving graphs, all Singleton values are eagerly
    // populated by `_resolve` before any factory or entry point is
    // called. Sync getters then just return the cached value
    // (`return this._x!`) — no `??=`, no per-call await, no virality.
    // Unscoped factories stay sync-fresh (an Unscoped binding can't be
    // async because `AsyncBindingNeedsSingletonComponent` rejects that
    // shape at validation time).
    for k in topo {
        let b = &graph.bindings[k];
        let type_string = type_string_for_binding(b);
        let factory = factory_name_for(k);
        let return_stmt = if matches!(b.scope, Scope::Singleton) {
            if graph_is_async {
                let field = cache_field_for(k);
                format!("return this.{field}!")
            } else {
                let field = cache_field_for(k);
                let body_expr = body_expr_for(k);
                format!("return this.{field} ??= {body_expr}")
            }
        } else {
            let body_expr = body_expr_for(k);
            format!("return {body_expr}")
        };

        let visibility = if expose_factories_for.contains(k) {
            ""
        } else {
            "private "
        };
        writeln!(
            s,
            "  {visibility}{factory}(): {type_string} {{ {return_stmt}; }}"
        )
        .expect("write to String");
    }

    // M12: emit the async resolution phase. Walks the topo array in
    // dep order and assigns each Singleton's resolved value to its
    // cache field. Async @Provides methods are awaited; sync factories
    // produce their value directly. Unscoped bindings and factory-param
    // bindings are NOT resolved here — they're either fresh-per-call
    // (Unscoped) or already supplied via the constructor (FactoryParam).
    if graph_is_async {
        writeln!(
            s,
            "  static async _resolve(d: {self_dagger}): Promise<void> {{"
        )
        .expect("write to String");
        for k in topo {
            let b = &graph.bindings[k];
            if !matches!(b.scope, Scope::Singleton) {
                continue;
            }
            let field = cache_field_for(k);
            // Re-render the body expression but rooted on `d.` (the
            // first arg) instead of `this.`, since `_resolve` is a
            // static method. We do this by string-rewriting `this.` to
            // `d.` — every dep call goes through `this.<get>()` so
            // this is a closed transform.
            let raw = body_expr_for(k).replace("this.", "d.");
            let prefix = if graph.binding_is_async(k) {
                "await "
            } else {
                ""
            };
            writeln!(s, "    d.{field} = {prefix}{raw};").expect("write to String");
        }
        s.push_str("  }\n");
    }

    for ep in entry_points {
        if sub_factory_method_names.contains(ep.name.as_str()) {
            // Skip — handled below as a subcomponent factory.
            continue;
        }
        // For Token entry points, use the binding's inner type (the return type
        // of the abstract method should be T, not Token<T, "name">).
        let type_string = if let Some(b) = graph.bindings.get(&ep.key) {
            type_string_for_binding(b)
        } else if graph.inherited_keys.contains(&ep.key) {
            // Inherited: fall back to type_string_of for display.
            type_string_of(&ep.key)
        } else {
            type_string_of(&ep.key)
        };
        let factory = factory_name_for(&ep.key);
        let body = if graph.inherited_keys.contains(&ep.key) {
            format!("this.parent.{factory}()")
        } else {
            format!("this.{factory}()")
        };
        writeln!(s, "  {}(): {} {{ return {}; }}", ep.name, type_string, body)
            .expect("write to String");
    }

    // Subcomponent factories: parent emits one method per child that
    // constructs the child Dagger with `this` as the parent reference.
    // M11: when factory_params is non-empty, the parent method takes
    // them as args and forwards them to the child constructor.
    // M12: when the child graph is async, the parent's factory method
    // returns Promise<Sub> and forwards through the child's static
    // `async create(...)` instead of constructing directly.
    for fact in subcomponent_factories {
        let sub_name = &fact.subcomponent.name;
        let sub_dagger = format!("Anvil{sub_name}");
        let params_sig = fact
            .factory_params
            .iter()
            .map(|fp| format!("{}: {}", fp.name, type_string_of(&fp.key)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut forwarded_args = String::from("this");
        for fp in &fact.factory_params {
            forwarded_args.push_str(", ");
            forwarded_args.push_str(&fp.name);
        }
        if fact.child_graph.is_async() {
            writeln!(
                s,
                "  async {}({}): Promise<{}> {{ return {}.create({}); }}",
                fact.method_name, params_sig, sub_name, sub_dagger, forwarded_args
            )
            .expect("write to String");
        } else {
            writeln!(
                s,
                "  {}({}): {} {{ return new {}({}); }}",
                fact.method_name, params_sig, sub_name, sub_dagger, forwarded_args
            )
            .expect("write to String");
        }
    }
}

/// Compute the call expression for `dep` from inside a factory body.
/// Inherited keys (only meaningful when `parent_dagger` is `Some`) route
/// through `this.parent.<getDep>()`; everything else uses `this.<getDep>()`.
fn dep_call(dep: &Key, graph: &DependencyGraph, parent_dagger: Option<&str>) -> String {
    let factory = factory_name_for(dep);
    if parent_dagger.is_some() && graph.inherited_keys.contains(dep) {
        format!("this.parent.{factory}()")
    } else {
        format!("this.{factory}()")
    }
}

/// `Heater` → `_heater`. Used for the lazy-cache private field on
/// singleton bindings. Lowercases only the first character so multi-word
/// class names (`HotPump` → `_hotPump`) keep their internal capitalization.
fn cache_field_name(class_name: &str) -> String {
    let mut chars = class_name.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::with_capacity(class_name.len() + 1);
            out.push('_');
            for c in first.to_lowercase() {
                out.push(c);
            }
            out.push_str(chars.as_str());
            out
        }
        None => "_".to_owned(),
    }
}

/// Compute a TS import specifier (no extension, forward slashes,
/// `./`-prefixed when relative-without-`..`).
///
/// `keep_ext = true` keeps `.ts` (used for the `// Source:` comment so
/// users can click through directly).
fn relative_ts_specifier(from_dir: &Path, to_abs: &Path, keep_ext: bool) -> String {
    let to_for_specifier = if keep_ext {
        to_abs.to_path_buf()
    } else {
        strip_ts_extension(to_abs)
    };
    let rel =
        relative_path(from_dir, &to_for_specifier).unwrap_or_else(|| to_for_specifier.clone());
    let raw = rel.to_string_lossy().replace('\\', "/");
    if raw.starts_with("./") || raw.starts_with("../") {
        raw
    } else {
        format!("./{raw}")
    }
}

/// Strip `.ts`, `.tsx`, or `.d.ts` from `path`. Other extensions (e.g.
/// `.json`) are kept since TS imports them with the extension.
fn strip_ts_extension(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    for suffix in [".d.ts", ".tsx", ".ts"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            return PathBuf::from(stripped);
        }
    }
    path.to_path_buf()
}

/// Compute a relative path from `from` to `to`, both expected to be absolute.
///
/// Returns `None` if either is not absolute (in practice this shouldn't
/// happen — M2 canonicalizes paths). Uses `..` to climb out of `from`'s
/// trailing components, then descends into `to`.
fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    if !from.is_absolute() || !to.is_absolute() {
        return None;
    }
    let from_comps: Vec<Component> = from.components().collect();
    let to_comps: Vec<Component> = to.components().collect();
    let mut i = 0;
    while i < from_comps.len() && i < to_comps.len() && from_comps[i] == to_comps[i] {
        i += 1;
    }
    let mut result = PathBuf::new();
    for _ in i..from_comps.len() {
        result.push("..");
    }
    for c in &to_comps[i..] {
        result.push(c.as_os_str());
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_name_prefixes_get() {
        let key = Key::class(
            anvil_core::ir::ModulePath::from_abs("/p/Heater.ts"),
            "Heater",
        );
        assert_eq!(factory_name_for(&key), "getHeater");
    }

    #[test]
    fn factory_name_for_set_uses_set_of_prefix() {
        let element = Key::class(
            anvil_core::ir::ModulePath::from_abs("/p/Plugin.ts"),
            "Plugin",
        );
        let key = Key::Set {
            element: Box::new(element),
        };
        assert_eq!(factory_name_for(&key), "getSetOfPlugin");
        assert_eq!(cache_field_for(&key), "_setOfPlugin");
        assert_eq!(type_string_of(&key), "Set<Plugin>");
    }

    #[test]
    fn strip_extension_handles_ts_tsx_dts() {
        assert_eq!(
            strip_ts_extension(Path::new("/a/b/c.ts")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            strip_ts_extension(Path::new("/a/b/c.tsx")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            strip_ts_extension(Path::new("/a/b/c.d.ts")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            strip_ts_extension(Path::new("/a/b/c.json")),
            PathBuf::from("/a/b/c.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn relative_specifier_same_dir() {
        let s = relative_ts_specifier(Path::new("/a/b"), Path::new("/a/b/heater.ts"), false);
        assert_eq!(s, "./heater");
    }

    #[cfg(unix)]
    #[test]
    fn relative_specifier_sibling_dir() {
        let s = relative_ts_specifier(Path::new("/a/b"), Path::new("/a/c/heater.ts"), false);
        assert_eq!(s, "../c/heater");
    }
}
