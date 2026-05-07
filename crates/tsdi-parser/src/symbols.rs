//! Cross-file symbol resolution.
//!
//! M2 layer on top of [`crate::decorators`]. This module:
//!
//! 1. Wraps [`oxc_resolver::Resolver`] with TypeScript-friendly defaults
//!    (extensions `.ts` / `.tsx` / `.d.ts` / `.js` / `.json`, optional
//!    `tsconfig.json` discovery).
//! 2. Normalizes the raw [`ModulePath`] values produced by M1 (import
//!    specifiers and the `SAME_FILE` sentinel) to absolute, OS-canonical
//!    filesystem paths. After normalization two `Key::Class` values compare
//!    equal iff they refer to the same source-file declaration.
//! 3. Builds a [`ProjectGraph`] from an entry file by transitively walking
//!    the import graph. Files reached through `node_modules` are *resolved*
//!    (so their `Key`s remain stable) but not *recursed* into — runtime
//!    libraries should not contribute bindings.
//!
//! In v0.1 every cross-file reference reaches the IR as a `Key::Class`, so
//! walking the IR after extraction is sufficient to discover every file
//! that may contain a binding the graph builder needs.
//!
//! Errors from this layer are bubbled up through [`SymbolError`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use oxc_resolver::{
    ResolveError, ResolveOptions, Resolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};
use thiserror::Error;
use tsdi_core::ir::{Binding, ClassRef, Key, ModulePath, ParsedFile};

use crate::map_source::{FileMap, MapResolveError, MapResolver};
use crate::ParseError;

/// A TypeScript-aware module resolver.
///
/// Constructed once and shared across every file in a project. Backed by
/// `oxc_resolver`, which honors `tsconfig.json` `paths` / `baseUrl` /
/// project references when configured.
pub struct ProjectResolver {
    inner: Resolver,
}

impl std::fmt::Debug for ProjectResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectResolver").finish_non_exhaustive()
    }
}

impl ProjectResolver {
    /// Build a resolver with TypeScript-friendly defaults.
    ///
    /// `tsconfig` is optional. When `Some`, it must point at an absolute
    /// `tsconfig.json` path; `paths` and `baseUrl` from that config will be
    /// honored.
    #[must_use]
    pub fn new(tsconfig: Option<PathBuf>) -> Self {
        let extensions = vec![
            ".ts".to_owned(),
            ".tsx".to_owned(),
            ".d.ts".to_owned(),
            ".js".to_owned(),
            ".jsx".to_owned(),
            ".json".to_owned(),
        ];
        let tsconfig = tsconfig.map(|config_file| {
            TsconfigDiscovery::Manual(TsconfigOptions {
                config_file,
                references: TsconfigReferences::Auto,
            })
        });
        let options = ResolveOptions {
            extensions,
            // Prefer source `.ts` over emitted `.js` when both exist next to
            // each other (a common pattern in checked-in `dist/` folders).
            extension_alias: vec![
                (".js".to_owned(), vec![".ts".to_owned(), ".js".to_owned()]),
                (
                    ".jsx".to_owned(),
                    vec![".tsx".to_owned(), ".jsx".to_owned()],
                ),
            ],
            condition_names: vec!["import".to_owned(), "require".to_owned(), "node".to_owned()],
            tsconfig,
            ..ResolveOptions::default()
        };
        Self {
            inner: Resolver::new(options),
        }
    }

    /// Resolve `specifier` as imported from `importer_dir`.
    ///
    /// `importer_dir` must be the **directory** of the importing file, as
    /// `oxc_resolver` expects.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`ResolveError`] verbatim. The CLI will dress
    /// these in `miette` diagnostics in M3.
    pub fn resolve(&self, importer_dir: &Path, specifier: &str) -> Result<PathBuf, ResolveError> {
        self.inner
            .resolve(importer_dir, specifier)
            .map(|r| r.full_path())
    }
}

/// All ways the cross-file resolver can fail.
#[derive(Debug, Error)]
pub enum SymbolError {
    /// A module specifier in `parsed`'s IR could not be resolved.
    #[error("failed to resolve {specifier:?} from {importer}: {source}")]
    Resolve {
        /// The specifier that failed.
        specifier: String,
        /// The file that imported it (for diagnostic context).
        importer: PathBuf,
        /// The underlying resolver error (boxed because `ResolveError` is large).
        #[source]
        source: Box<ResolveError>,
    },
    /// A specifier could not be resolved against an in-memory file
    /// map (M13). Surfaces the exact same shape as the native
    /// [`Self::Resolve`] error, but sources from
    /// [`MapResolveError`] instead of `oxc_resolver`'s error type so
    /// the WASM build can avoid pulling `oxc_resolver` in.
    #[error("failed to resolve {specifier:?} from {importer}: {source}")]
    MapResolve {
        /// The specifier that failed.
        specifier: String,
        /// The file that imported it.
        importer: PathBuf,
        /// The underlying map-resolver error.
        #[source]
        source: MapResolveError,
    },
    /// An entry path is missing or has no parent directory.
    #[error("invalid entry path {0}")]
    BadEntry(PathBuf),
    /// A file the walker tried to parse failed to parse.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Aggregated parse output across an entire reachable import graph.
///
/// Built by [`ProjectGraph::build_from_entry`]. Every [`ModulePath`] inside
/// the contained [`ParsedFile`]s is **absolute**.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// Map from absolute file path → that file's normalized [`ParsedFile`].
    pub files: BTreeMap<PathBuf, ParsedFile>,
}

impl ProjectGraph {
    /// Walk the import graph starting from `entry`, parsing every reachable
    /// `.ts` / `.tsx` source file and normalizing its IR's [`ModulePath`]s.
    ///
    /// `entry` must be an existing file path; canonicalization is applied
    /// so that the resulting graph keys are stable regardless of how the
    /// caller spelled the path.
    ///
    /// Files reachable only through `node_modules` are not walked into —
    /// runtime packages don't contribute DI bindings in v0.1.
    pub fn build_from_entry(entry: &Path, resolver: &ProjectResolver) -> Result<Self, SymbolError> {
        let entry = canonicalize_or_keep(entry);
        let mut graph = ProjectGraph::default();
        let mut queue: Vec<PathBuf> = vec![entry];
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            // Skip non-source files (`.d.ts`, `.js`, `.json`, anything in
            // node_modules). They're allowed as resolution targets but the
            // parser would either reject them or contribute no bindings.
            if !is_source_ts(&path) || in_node_modules(&path) {
                continue;
            }

            let mut parsed = crate::parse_file(&path)?;
            let abs_path = path.clone();
            let dir = abs_path
                .parent()
                .ok_or_else(|| SymbolError::BadEntry(abs_path.clone()))?;
            let mut next: Vec<PathBuf> = Vec::new();
            normalize_parsed(&mut parsed, &abs_path, dir, resolver, &mut next)?;
            // Update the IR's path field to the canonical form so
            // diagnostics (and the SAME_FILE rewrite) line up.
            parsed.path = abs_path.to_string_lossy().into_owned();
            graph.files.insert(abs_path, parsed);
            queue.extend(next);
        }
        Ok(graph)
    }

    /// Walk the import graph starting from `entry`, sourcing every
    /// file's contents from the supplied [`FileMap`] instead of the
    /// real filesystem (M13). Mirrors [`Self::build_from_entry`] in
    /// every other respect — same canonicalization shape, same
    /// node_modules-as-leaf rule, same per-file IR normalization.
    ///
    /// Built for the WASM crate so the same codegen pipeline works
    /// in browsers / Workers / Vite's in-process mode without
    /// touching `std::fs`. Native callers should keep using
    /// [`Self::build_from_entry`] — it has a more capable resolver
    /// (full `oxc_resolver` semantics) and avoids materializing the
    /// project's source into RAM up front.
    pub fn build_from_map(
        entry: &Path,
        files: &FileMap,
        resolver: &MapResolver,
    ) -> Result<Self, SymbolError> {
        let entry = crate::map_source::normalize(entry);
        let mut graph = ProjectGraph::default();
        let mut queue: Vec<PathBuf> = vec![entry];
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            if !is_source_ts(&path) || in_node_modules(&path) {
                continue;
            }
            let source = files.read(&path).ok_or_else(|| {
                SymbolError::Parse(ParseError::Io {
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file {} not present in FileMap", path.display()),
                    ),
                })
            })?;
            let mut parsed = crate::parse_source(source, &path.display().to_string())?;
            let dir = path
                .parent()
                .ok_or_else(|| SymbolError::BadEntry(path.clone()))?;
            let mut next: Vec<PathBuf> = Vec::new();
            normalize_parsed_with_map(&mut parsed, &path, dir, resolver, files, &mut next)?;
            parsed.path = path.to_string_lossy().into_owned();
            graph.files.insert(path, parsed);
            queue.extend(next);
        }
        Ok(graph)
    }
}

/// Rewrite every [`ModulePath`] in `parsed` to an absolute, canonical
/// filesystem path. Any newly-discovered absolute paths the caller should
/// recurse into are appended to `discovered`.
fn normalize_parsed(
    parsed: &mut ParsedFile,
    self_path: &Path,
    self_dir: &Path,
    resolver: &ProjectResolver,
    discovered: &mut Vec<PathBuf>,
) -> Result<(), SymbolError> {
    // Cache resolutions per (raw specifier) to avoid re-hitting the file
    // system for the same specifier inside a single file.
    let mut cache: HashMap<String, PathBuf> = HashMap::new();
    let self_path_string = self_path.to_string_lossy().into_owned();

    let mut resolve_one =
        |mp: &mut ModulePath, discovered: &mut Vec<PathBuf>| -> Result<(), SymbolError> {
            // Same-file sentinel: rewrite `abs` to the importing file's
            // canonical path. `original` stays `None` (there's no
            // user-written specifier for a same-file ref).
            if mp.abs == ModulePath::SAME_FILE {
                mp.abs.clone_from(&self_path_string);
                return Ok(());
            }
            // For real specifiers, only `abs` gets rewritten — `original`
            // is what the user wrote, which codegen needs verbatim for
            // node_modules imports.
            let abs = if let Some(cached) = cache.get(&mp.abs) {
                cached.clone()
            } else {
                let abs =
                    resolver
                        .resolve(self_dir, &mp.abs)
                        .map_err(|source| SymbolError::Resolve {
                            specifier: mp.abs.clone(),
                            importer: self_path.to_path_buf(),
                            source: Box::new(source),
                        })?;
                let abs = canonicalize_or_keep(&abs);
                cache.insert(mp.abs.clone(), abs.clone());
                abs
            };
            mp.abs = abs.to_string_lossy().into_owned();
            // Only recurse into project source files.
            if is_source_ts(&abs) && !in_node_modules(&abs) {
                discovered.push(abs);
            }
            Ok(())
        };

    for m in &mut parsed.modules {
        rewrite_classref(&mut m.class, &mut resolve_one, discovered)?;
        m.source.path.clone_from(&self_path_string);
        for b in &mut m.provides {
            rewrite_binding(b, &mut resolve_one, discovered)?;
            b.source.path.clone_from(&self_path_string);
        }
    }
    for c in &mut parsed.components {
        rewrite_classref(&mut c.class, &mut resolve_one, discovered)?;
        c.source.path.clone_from(&self_path_string);
        for cm in &mut c.modules {
            rewrite_classref(cm, &mut resolve_one, discovered)?;
        }
        for ep in &mut c.entry_points {
            rewrite_key(&mut ep.key, &mut resolve_one, discovered)?;
            ep.source.path.clone_from(&self_path_string);
            // M11: factory-param keys must be normalized too so they
            // compare equal to the same Key minted elsewhere.
            for fp in &mut ep.factory_params {
                rewrite_key(&mut fp.key, &mut resolve_one, discovered)?;
                fp.source.path.clone_from(&self_path_string);
            }
        }
    }
    for s in &mut parsed.subcomponents {
        rewrite_classref(&mut s.class, &mut resolve_one, discovered)?;
        s.source.path.clone_from(&self_path_string);
        for cm in &mut s.modules {
            rewrite_classref(cm, &mut resolve_one, discovered)?;
        }
        for ep in &mut s.entry_points {
            rewrite_key(&mut ep.key, &mut resolve_one, discovered)?;
            ep.source.path.clone_from(&self_path_string);
            for fp in &mut ep.factory_params {
                rewrite_key(&mut fp.key, &mut resolve_one, discovered)?;
                fp.source.path.clone_from(&self_path_string);
            }
        }
    }
    for b in &mut parsed.inject_classes {
        rewrite_binding(b, &mut resolve_one, discovered)?;
        b.source.path.clone_from(&self_path_string);
    }
    Ok(())
}

/// M13: WASM-side counterpart to [`normalize_parsed`]. Same control
/// flow (rewrite every embedded `ModulePath`, queue project-internal
/// discoveries) but consults a [`FileMap`] + [`MapResolver`] instead
/// of `oxc_resolver` + `std::fs`. The two functions diverge only in
/// the `resolve_one` closure they use; everything below it is shared
/// via the existing `rewrite_*` helpers.
fn normalize_parsed_with_map(
    parsed: &mut ParsedFile,
    self_path: &Path,
    self_dir: &Path,
    resolver: &MapResolver,
    files: &FileMap,
    discovered: &mut Vec<PathBuf>,
) -> Result<(), SymbolError> {
    let mut cache: HashMap<String, PathBuf> = HashMap::new();
    let self_path_string = self_path.to_string_lossy().into_owned();

    let mut resolve_one =
        |mp: &mut ModulePath, discovered: &mut Vec<PathBuf>| -> Result<(), SymbolError> {
            if mp.abs == ModulePath::SAME_FILE {
                mp.abs.clone_from(&self_path_string);
                return Ok(());
            }
            let abs = if let Some(cached) = cache.get(&mp.abs) {
                cached.clone()
            } else {
                let abs = resolver
                    .resolve(self_dir, &mp.abs, files)
                    .map_err(|source| SymbolError::MapResolve {
                        specifier: mp.abs.clone(),
                        importer: self_path.to_path_buf(),
                        source,
                    })?;
                let abs = crate::map_source::normalize(&abs);
                cache.insert(mp.abs.clone(), abs.clone());
                abs
            };
            mp.abs = abs.to_string_lossy().into_owned();
            if is_source_ts(&abs) && !in_node_modules(&abs) {
                discovered.push(abs);
            }
            Ok(())
        };

    for m in &mut parsed.modules {
        rewrite_classref(&mut m.class, &mut resolve_one, discovered)?;
        m.source.path.clone_from(&self_path_string);
        for b in &mut m.provides {
            rewrite_binding(b, &mut resolve_one, discovered)?;
            b.source.path.clone_from(&self_path_string);
        }
    }
    for c in &mut parsed.components {
        rewrite_classref(&mut c.class, &mut resolve_one, discovered)?;
        c.source.path.clone_from(&self_path_string);
        for cm in &mut c.modules {
            rewrite_classref(cm, &mut resolve_one, discovered)?;
        }
        for ep in &mut c.entry_points {
            rewrite_key(&mut ep.key, &mut resolve_one, discovered)?;
            ep.source.path.clone_from(&self_path_string);
            for fp in &mut ep.factory_params {
                rewrite_key(&mut fp.key, &mut resolve_one, discovered)?;
                fp.source.path.clone_from(&self_path_string);
            }
        }
    }
    for s in &mut parsed.subcomponents {
        rewrite_classref(&mut s.class, &mut resolve_one, discovered)?;
        s.source.path.clone_from(&self_path_string);
        for cm in &mut s.modules {
            rewrite_classref(cm, &mut resolve_one, discovered)?;
        }
        for ep in &mut s.entry_points {
            rewrite_key(&mut ep.key, &mut resolve_one, discovered)?;
            ep.source.path.clone_from(&self_path_string);
            for fp in &mut ep.factory_params {
                rewrite_key(&mut fp.key, &mut resolve_one, discovered)?;
                fp.source.path.clone_from(&self_path_string);
            }
        }
    }
    for b in &mut parsed.inject_classes {
        rewrite_binding(b, &mut resolve_one, discovered)?;
        b.source.path.clone_from(&self_path_string);
    }
    Ok(())
}

fn rewrite_binding(
    b: &mut Binding,
    resolve_one: &mut impl FnMut(&mut ModulePath, &mut Vec<PathBuf>) -> Result<(), SymbolError>,
    discovered: &mut Vec<PathBuf>,
) -> Result<(), SymbolError> {
    rewrite_key(&mut b.key, resolve_one, discovered)?;
    match &mut b.provider {
        tsdi_core::ir::Provider::InjectCtor { class }
        | tsdi_core::ir::Provider::ProvidesMethod { module: class, .. } => {
            rewrite_classref(class, resolve_one, discovered)?;
        }
        tsdi_core::ir::Provider::Binds { target } => {
            rewrite_key(target, resolve_one, discovered)?;
        }
        tsdi_core::ir::Provider::SetMultibinding { contributors } => {
            for c in contributors {
                rewrite_classref(&mut c.module, resolve_one, discovered)?;
                for d in &mut c.deps {
                    rewrite_key(d, resolve_one, discovered)?;
                }
            }
        }
        tsdi_core::ir::Provider::FactoryParam { .. } => {
            // FactoryParam bindings are graph-synthesized — the parser
            // never produces one and thus never feeds it through
            // resolution. Match arm exists for completeness against
            // future code paths that might run rewrite_binding on a
            // child graph's bindings.
        }
    }
    for d in &mut b.deps {
        rewrite_key(d, resolve_one, discovered)?;
    }
    Ok(())
}

fn rewrite_key(
    key: &mut Key,
    resolve_one: &mut impl FnMut(&mut ModulePath, &mut Vec<PathBuf>) -> Result<(), SymbolError>,
    discovered: &mut Vec<PathBuf>,
) -> Result<(), SymbolError> {
    match key {
        Key::Class { module, .. } => resolve_one(module, discovered),
        Key::Set { element } => rewrite_key(element, resolve_one, discovered),
    }
}

fn rewrite_classref(
    cr: &mut ClassRef,
    resolve_one: &mut impl FnMut(&mut ModulePath, &mut Vec<PathBuf>) -> Result<(), SymbolError>,
    discovered: &mut Vec<PathBuf>,
) -> Result<(), SymbolError> {
    resolve_one(&mut cr.module, discovered)
}

fn is_source_ts(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if s.ends_with(".d.ts") {
        return false;
    }
    matches!(p.extension().and_then(|e| e.to_str()), Some("ts" | "tsx"))
}

fn in_node_modules(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new("node_modules"))
}

fn canonicalize_or_keep(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_source_ts_filters() {
        assert!(is_source_ts(Path::new("/x/foo.ts")));
        assert!(is_source_ts(Path::new("/x/foo.tsx")));
        assert!(!is_source_ts(Path::new("/x/foo.d.ts")));
        assert!(!is_source_ts(Path::new("/x/foo.js")));
        assert!(!is_source_ts(Path::new("/x/foo.json")));
    }

    #[test]
    fn node_modules_detected() {
        assert!(in_node_modules(Path::new(
            "/proj/node_modules/foo/index.ts"
        )));
        assert!(!in_node_modules(Path::new("/proj/src/foo.ts")));
    }
}
