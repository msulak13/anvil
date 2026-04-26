//! Emit one `*.tsdi.ts` file for a single `@Component`.
//!
//! Pipeline:
//!
//! 1. Run [`tsdi_core::graph::build_and_validate`] over the component +
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

use tsdi_core::graph::{build_and_validate, DependencyGraph, GraphInput};
use tsdi_core::ir::{Binding, ClassRef, ComponentDecl, Key, ModuleDecl, Provider, Scope};

use crate::{banner_for, EmitError, Result};

/// Emit the contents of `<component>.tsdi.ts` for one component.
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
    version: &str,
) -> Result<String> {
    let (graph, diagnostics) = build_and_validate(GraphInput {
        component,
        modules,
        inject_classes,
    });
    if !diagnostics.is_empty() {
        return Err(EmitError::Invalid(diagnostics));
    }

    let component_path = PathBuf::from(&component.class.module.0);
    let out_dir = component_path
        .parent()
        .ok_or_else(|| EmitError::BadComponentPath(component_path.display().to_string()))?
        .to_path_buf();

    let bindings_topo = topo_order(&graph);
    let imports = collect_imports(&out_dir, component, &graph, &bindings_topo);
    let body = build_ts_source(component, &graph, &bindings_topo, &imports);

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

/// Return the class name from a `Key`. v0.1 only has `Key::Class`.
fn class_name_of(key: &Key) -> &str {
    let Key::Class { name, .. } = key;
    name
}

fn key_module(key: &Key) -> &str {
    let Key::Class { module, .. } = key;
    &module.0
}

/// Stable comparison key for ties: `"Name@module"`.
fn lex_key(k: &Key) -> String {
    format!("{}@{}", class_name_of(k), key_module(k))
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
    }
    out.push(k.clone());
}

/// Map `specifier` (e.g. `"./heater"`) → set of names imported from it.
type ImportMap = BTreeMap<String, BTreeSet<String>>;

fn collect_imports(
    out_dir: &Path,
    component: &ComponentDecl,
    graph: &DependencyGraph,
    topo: &[Key],
) -> ImportMap {
    let mut imports: ImportMap = BTreeMap::new();
    add_classref_import(&mut imports, out_dir, &component.class);
    for k in topo {
        let b = &graph.bindings[k];
        // The binding's produced type:
        let key_classref = ClassRef {
            module: match &b.key {
                Key::Class { module, .. } => module.clone(),
            },
            name: class_name_of(&b.key).to_owned(),
        };
        add_classref_import(&mut imports, out_dir, &key_classref);
        match &b.provider {
            Provider::InjectCtor { class } => {
                add_classref_import(&mut imports, out_dir, class);
            }
            Provider::ProvidesMethod { module, .. } => {
                add_classref_import(&mut imports, out_dir, module);
            }
        }
    }
    imports
}

fn add_classref_import(imports: &mut ImportMap, out_dir: &Path, cref: &ClassRef) {
    let abs = PathBuf::from(&cref.module.0);
    let specifier = relative_ts_specifier(out_dir, &abs, /* keep_ext */ false);
    imports
        .entry(specifier)
        .or_default()
        .insert(cref.name.clone());
}

fn build_ts_source(
    component: &ComponentDecl,
    graph: &DependencyGraph,
    bindings_topo: &[Key],
    imports: &ImportMap,
) -> String {
    let mut s = String::new();

    for (specifier, names) in imports {
        let names_csv: Vec<&str> = names.iter().map(String::as_str).collect();
        writeln!(
            s,
            "import {{ {} }} from \"{}\";",
            names_csv.join(", "),
            specifier
        )
        .expect("write to String");
    }
    s.push('\n');

    let comp_name = &component.class.name;
    let dagger_name = format!("Dagger{comp_name}");

    writeln!(s, "export class {dagger_name} extends {comp_name} {{").expect("write to String");

    // First: declare cache fields for every singleton binding, in topo order
    // so the field declarations match the factory order users will read.
    for k in bindings_topo {
        let b = &graph.bindings[k];
        if matches!(b.scope, Scope::Singleton) {
            let class_name = class_name_of(k);
            let field = cache_field_name(class_name);
            writeln!(s, "  private {field}: {class_name} | undefined;").expect("write to String");
        }
    }

    for k in bindings_topo {
        let b = &graph.bindings[k];
        let class_name = class_name_of(k);
        let factory = factory_name(class_name);
        let dep_args = b
            .deps
            .iter()
            .map(|d| format!("this.{}()", factory_name(class_name_of(d))))
            .collect::<Vec<_>>()
            .join(", ");

        let body_expr = match &b.provider {
            Provider::InjectCtor { class } => format!("new {}({})", class.name, dep_args),
            Provider::ProvidesMethod { module, method } => {
                format!("{}.{}({})", module.name, method, dep_args)
            }
        };

        let return_stmt = if matches!(b.scope, Scope::Singleton) {
            let field = cache_field_name(class_name);
            // `??=` returns the assigned value, so this is shorter than an
            // `if (this._x === undefined) ...; return this._x` chain.
            format!("return this.{field} ??= {body_expr}")
        } else {
            format!("return {body_expr}")
        };

        writeln!(
            s,
            "  private {factory}(): {class_name} {{ {return_stmt}; }}"
        )
        .expect("write to String");
    }

    for ep in &component.entry_points {
        let class_name = class_name_of(&ep.key);
        let factory = factory_name(class_name);
        writeln!(
            s,
            "  {}(): {} {{ return this.{}(); }}",
            ep.name, class_name, factory
        )
        .expect("write to String");
    }

    writeln!(
        s,
        "  static create(): {comp_name} {{ return new {dagger_name}(); }}"
    )
    .expect("write to String");
    s.push_str("}\n");
    writeln!(
        s,
        "export function create{comp_name}(): {comp_name} {{ return {dagger_name}.create(); }}"
    )
    .expect("write to String");
    s
}

/// `Heater` → `getHeater`. The class name is already `UpperCamel` by TS
/// convention, so we just prefix `get`.
fn factory_name(class_name: &str) -> String {
    format!("get{class_name}")
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
        assert_eq!(factory_name("Heater"), "getHeater");
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
