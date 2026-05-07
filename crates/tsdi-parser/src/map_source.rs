//! In-memory file source + resolver for environments without
//! filesystem access (WASM, browser, isolated test fixtures).
//!
//! Mirrors the surface of [`crate::symbols::ProjectResolver`] but
//! reads from a pre-supplied `HashMap<PathBuf, String>` instead of
//! `std::fs`. The resolver is intentionally minimal — it covers the
//! cases tsdi actually exercises in v0.2:
//!
//! - **Relative imports** (`./foo`, `../bar`) — resolved against the
//!   importer's directory, with extension probing for `.ts`, `.tsx`,
//!   `.d.ts`, `.js`, `.jsx`, `.json`.
//! - **Tsconfig `paths` aliases** (`@/foo` → `./src/foo`) — applied
//!   when a tsconfig is supplied and a specifier matches an alias
//!   pattern.
//! - **Bare specifiers** (`tsdi`, `express`, `@scope/pkg`) — looked up
//!   under any `node_modules/<spec>` entry the host pre-loaded into
//!   the file map. Hosts that already have bundler-grade resolution
//!   (Vite, esbuild, etc.) can sidestep this by pre-resolving every
//!   import path-side and passing tsdi an absolute-path-keyed map.
//!
//! Anything more sophisticated (`browser` field, conditional exports,
//! workspace overrides) is out of scope — those are the bundler's
//! job, and the bundler is what hands us the file map in the first
//! place.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// A minimal `Result` for resolver errors that isn't bound to
/// `oxc_resolver::ResolveError` (which won't compile under WASM
/// because of its dependency tree). This is the WASM-side analogue
/// of the native `SymbolError::Resolve` flavour.
#[derive(Debug, Error)]
pub enum MapResolveError {
    /// The specifier was relative but couldn't be matched against any
    /// entry in the file map (after extension probing).
    #[error("could not resolve relative specifier {specifier:?} from {importer_dir:?}")]
    RelativeNotFound {
        /// The specifier that failed.
        specifier: String,
        /// The directory the import was resolved from.
        importer_dir: PathBuf,
    },
    /// The specifier was bare (e.g. `tsdi`, `express`) and no
    /// matching `node_modules/<spec>/...` entry existed in the file
    /// map.
    #[error("could not resolve bare specifier {specifier:?} (no matching node_modules entry in the file map)")]
    BareNotFound {
        /// The specifier that failed.
        specifier: String,
    },
}

/// In-memory file map. Keys are absolute paths; values are file
/// contents. Borrowed by [`MapResolver`] to do its lookups.
#[derive(Debug, Clone, Default)]
pub struct FileMap {
    files: HashMap<PathBuf, String>,
}

impl FileMap {
    /// Build a map from an iterator of `(path, contents)` pairs.
    /// Paths are stored verbatim — caller is responsible for passing
    /// absolute, normalized paths so identity comparisons line up.
    pub fn from_pairs<I, P, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (P, S)>,
        P: Into<PathBuf>,
        S: Into<String>,
    {
        let mut files = HashMap::new();
        for (k, v) in pairs {
            files.insert(k.into(), v.into());
        }
        Self { files }
    }

    /// Read a file's contents by absolute path. Returns `None` if
    /// the path isn't in the map.
    #[must_use]
    pub fn read(&self, path: &Path) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Whether the map contains an entry for the given path.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    /// Iterate over every entry in the map. Order is unspecified.
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.files.iter().map(|(k, v)| (k.as_path(), v.as_str()))
    }

    /// Number of entries in the map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// One alias entry from a tsconfig's `compilerOptions.paths` field.
///
/// `pattern` and `target` are stored as glob-style strings where `*`
/// is the only allowed wildcard (matching tsconfig's own grammar).
/// `base_dir` is the absolute directory the relative `target` paths
/// resolve against (typically the tsconfig's own dir, or its
/// `baseUrl`).
#[derive(Debug, Clone)]
pub struct PathAlias {
    /// The alias pattern (e.g. `"@/*"`).
    pub pattern: String,
    /// The first matching target template. tsconfig allows multiple
    /// targets per pattern; we honor only the first to keep behavior
    /// deterministic.
    pub target: String,
    /// Where the relative target paths live.
    pub base_dir: PathBuf,
}

impl PathAlias {
    /// Try to apply this alias to a specifier. Returns the rewritten
    /// path (relative or absolute, depending on `target`'s form) when
    /// the pattern matches.
    fn apply(&self, specifier: &str) -> Option<PathBuf> {
        let star_pos = self.pattern.find('*');
        match star_pos {
            None => {
                // Exact-match pattern, e.g. "@app".
                if specifier == self.pattern {
                    Some(self.base_dir.join(&self.target))
                } else {
                    None
                }
            }
            Some(idx) => {
                let prefix = &self.pattern[..idx];
                let suffix = &self.pattern[idx + 1..];
                if !specifier.starts_with(prefix) || !specifier.ends_with(suffix) {
                    return None;
                }
                let captured = &specifier[prefix.len()..specifier.len() - suffix.len()];
                let target_filled = self.target.replace('*', captured);
                Some(self.base_dir.join(target_filled))
            }
        }
    }
}

/// A minimal, file-map-backed resolver.
///
/// Constructed once per build and reused across every file. Holds an
/// owning copy of the alias table; the file map is borrowed at
/// resolve time to keep large file blobs out of the resolver's
/// memory footprint.
#[derive(Debug, Clone, Default)]
pub struct MapResolver {
    aliases: Vec<PathAlias>,
}

/// Extensions tried when probing for an unsuffixed relative import.
/// Order matters: `.ts` wins over `.js` so source survives next to
/// emitted output (matching the [`crate::symbols::ProjectResolver`]
/// extension-alias behaviour).
const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".jsx", ".js", ".json"];

/// Filenames probed when a specifier resolves to a directory.
const INDEX_FILES: &[&str] = &["index.ts", "index.tsx", "index.d.ts", "index.js"];

impl MapResolver {
    /// Build a resolver with no path aliases. Use [`Self::with_aliases`]
    /// to add tsconfig `paths` mappings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a resolver with the given alias list. Aliases are tried
    /// in order; the first match wins.
    #[must_use]
    pub fn with_aliases(aliases: Vec<PathAlias>) -> Self {
        Self { aliases }
    }

    /// Resolve `specifier` (an import path as written in source)
    /// against `importer_dir` (the directory containing the file
    /// that issued the import) using `files` as the universe of
    /// known files.
    ///
    /// # Errors
    ///
    /// Returns [`MapResolveError`] when no entry in `files` matches —
    /// either no aliased / relative path resolves, or no
    /// `node_modules/<spec>` entry exists for a bare specifier.
    pub fn resolve(
        &self,
        importer_dir: &Path,
        specifier: &str,
        files: &FileMap,
    ) -> Result<PathBuf, MapResolveError> {
        // 1. Tsconfig paths aliases. Try each in turn — first match wins.
        for alias in &self.aliases {
            if let Some(candidate) = alias.apply(specifier) {
                if let Some(found) = probe_with_extensions(&candidate, files) {
                    return Ok(found);
                }
            }
        }

        // 2. Relative imports (./foo, ../foo, /abs).
        if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
        {
            let candidate = if specifier.starts_with('/') {
                PathBuf::from(specifier)
            } else {
                normalize(&importer_dir.join(specifier))
            };
            if let Some(found) = probe_with_extensions(&candidate, files) {
                return Ok(found);
            }
            return Err(MapResolveError::RelativeNotFound {
                specifier: specifier.to_owned(),
                importer_dir: importer_dir.to_path_buf(),
            });
        }

        // 3. Bare specifiers. Look for any file whose absolute path
        //    contains `node_modules/<spec>/...` as a substring. We
        //    pick the entry-point file (`index.{ts,d.ts,js}` or the
        //    package's `main`/types) by scanning candidates rather
        //    than re-parsing package.json — bundler-side hosts can
        //    pre-resolve to an absolute path if they want surgical
        //    control.
        let needle_unix = format!("node_modules/{specifier}/");
        let needle_win = needle_unix.replace('/', "\\");
        for (path, _) in files.iter() {
            let p = path.to_string_lossy();
            if p.contains(&needle_unix) || p.contains(&needle_win) {
                // Prefer `index.{ts,d.ts,js}` over arbitrary other
                // files inside the package — that's what tsc does
                // for bare specifiers without an `exports` field.
                if INDEX_FILES.iter().any(|name| p.ends_with(name)) {
                    return Ok(path.to_path_buf());
                }
            }
        }
        // Fall back to any file under the package directory if no
        // explicit index was provided. Helps when the host ships a
        // single .d.ts at an arbitrary path.
        for (path, _) in files.iter() {
            let p = path.to_string_lossy();
            if p.contains(&needle_unix) || p.contains(&needle_win) {
                return Ok(path.to_path_buf());
            }
        }

        Err(MapResolveError::BareNotFound {
            specifier: specifier.to_owned(),
        })
    }
}

/// Try the candidate path verbatim, then with each tsdi-supported
/// extension, then probe `<dir>/index.{ts,d.ts,js}`. Returns the
/// first match found in `files`.
fn probe_with_extensions(candidate: &Path, files: &FileMap) -> Option<PathBuf> {
    if files.contains(candidate) {
        return Some(candidate.to_path_buf());
    }
    for ext in EXTENSIONS {
        let mut buf = candidate.as_os_str().to_owned();
        buf.push(ext);
        let with_ext = PathBuf::from(buf);
        if files.contains(&with_ext) {
            return Some(with_ext);
        }
    }
    for index in INDEX_FILES {
        let dir_index = candidate.join(index);
        if files.contains(&dir_index) {
            return Some(dir_index);
        }
    }
    None
}

/// Normalize a path by collapsing `.` and `..` components without
/// touching the filesystem. The native `std::fs::canonicalize` does
/// this *and* resolves symlinks; in WASM (or any host that hasn't
/// fed us symlinks), purely textual normalization is sufficient.
///
/// **Windows correctness:** `Path::components()` yields a Windows
/// absolute path's drive prefix (`C:`) and root separator (`\`) as
/// two distinct components. Pushing them through `PathBuf::push` one
/// at a time silently *replaces* the prefix when the lone separator
/// is pushed — `PathBuf::push("\\")` treats it as an absolute path
/// and overwrites everything before it. To preserve the original
/// absolute prefix exactly, we slice it out of the source path's
/// underlying `OsStr` instead of round-tripping it through
/// `PathBuf::push`. Only the trailing `Normal`/`ParentDir`/`CurDir`
/// components actually need collapsing.
#[must_use]
pub fn normalize(p: &Path) -> PathBuf {
    // Compute how many bytes of the original path string are taken
    // up by the prefix + root components. We keep them as a single
    // literal so we never round-trip through PathBuf::push.
    let mut prefix_byte_len = 0usize;
    for c in p.components() {
        match c {
            Component::Prefix(pc) => prefix_byte_len += pc.as_os_str().len(),
            Component::RootDir => prefix_byte_len += 1,
            _ => break,
        }
    }
    let original_lossy = p.as_os_str().to_string_lossy();
    let prefix_str = &original_lossy[..prefix_byte_len.min(original_lossy.len())];
    let mut out = if prefix_str.is_empty() {
        PathBuf::new()
    } else {
        PathBuf::from(prefix_str)
    };

    for c in p.components() {
        match c {
            // Prefix + RootDir already handled above.
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Already at the root; preserve the `..` so the
                    // caller's "did this resolve outside the project"
                    // checks still work. In practice this almost
                    // never happens — bundlers don't issue imports
                    // that escape the project root.
                    out.push("..");
                }
            }
            Component::Normal(name) => out.push(name),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> FileMap {
        FileMap::from_pairs(
            pairs
                .iter()
                .map(|(k, v)| (PathBuf::from(*k), (*v).to_owned())),
        )
    }

    #[test]
    fn relative_import_with_extension_probe() {
        let files = map(&[("/proj/src/heater.ts", ""), ("/proj/src/main.ts", "")]);
        let r = MapResolver::new();
        let got = r
            .resolve(Path::new("/proj/src"), "./heater", &files)
            .unwrap();
        assert_eq!(got, PathBuf::from("/proj/src/heater.ts"));
    }

    #[test]
    fn relative_dotdot_climbs_one_level() {
        let files = map(&[("/proj/shared/util.ts", ""), ("/proj/src/main.ts", "")]);
        let r = MapResolver::new();
        let got = r
            .resolve(Path::new("/proj/src"), "../shared/util", &files)
            .unwrap();
        assert_eq!(got, PathBuf::from("/proj/shared/util.ts"));
    }

    #[test]
    fn directory_import_falls_through_to_index() {
        let files = map(&[("/proj/src/utils/index.ts", "")]);
        let r = MapResolver::new();
        let got = r
            .resolve(Path::new("/proj/src"), "./utils", &files)
            .unwrap();
        assert_eq!(got, PathBuf::from("/proj/src/utils/index.ts"));
    }

    #[test]
    fn missing_relative_import_errors() {
        let files = map(&[]);
        let r = MapResolver::new();
        let err = r
            .resolve(Path::new("/proj/src"), "./missing", &files)
            .unwrap_err();
        assert!(matches!(err, MapResolveError::RelativeNotFound { .. }));
    }

    #[test]
    fn tsconfig_paths_alias_with_star() {
        let files = map(&[("/proj/src/services/auth.ts", "")]);
        let r = MapResolver::with_aliases(vec![PathAlias {
            pattern: "@/*".to_owned(),
            target: "src/*".to_owned(),
            base_dir: PathBuf::from("/proj"),
        }]);
        let got = r
            .resolve(Path::new("/proj/src"), "@/services/auth", &files)
            .unwrap();
        assert_eq!(got, PathBuf::from("/proj/src/services/auth.ts"));
    }

    #[test]
    fn tsconfig_paths_alias_exact_match() {
        let files = map(&[("/proj/lib/runtime.ts", "")]);
        let r = MapResolver::with_aliases(vec![PathAlias {
            pattern: "@app".to_owned(),
            target: "lib/runtime".to_owned(),
            base_dir: PathBuf::from("/proj"),
        }]);
        let got = r.resolve(Path::new("/proj/src"), "@app", &files).unwrap();
        assert_eq!(got, PathBuf::from("/proj/lib/runtime.ts"));
    }

    #[test]
    fn bare_specifier_finds_node_modules_index() {
        let files = map(&[
            (
                "/proj/node_modules/express/index.d.ts",
                "export class Application {}",
            ),
            ("/proj/node_modules/express/package.json", "{}"),
        ]);
        let r = MapResolver::new();
        let got = r
            .resolve(Path::new("/proj/src"), "express", &files)
            .unwrap();
        assert_eq!(got, PathBuf::from("/proj/node_modules/express/index.d.ts"));
    }

    #[test]
    fn bare_specifier_not_found() {
        let files = map(&[]);
        let r = MapResolver::new();
        let err = r
            .resolve(Path::new("/proj/src"), "missing-pkg", &files)
            .unwrap_err();
        assert!(matches!(err, MapResolveError::BareNotFound { .. }));
    }

    #[test]
    fn normalize_collapses_parent_segments() {
        assert_eq!(
            normalize(Path::new("/proj/src/../lib/foo")),
            PathBuf::from("/proj/lib/foo")
        );
        assert_eq!(
            normalize(Path::new("/proj/./src/./main.ts")),
            PathBuf::from("/proj/src/main.ts")
        );
    }

    #[test]
    #[cfg(windows)]
    fn normalize_preserves_windows_absolute_prefix() {
        // Regression: PathBuf::push("\\") treats the lone separator
        // as absolute and replaces an existing C: prefix. Verify the
        // prefix survives normalization.
        let input = Path::new(r"C:\Users\test\proj\src\..\lib\foo.ts");
        let got = normalize(input);
        assert!(
            got.starts_with(r"C:\"),
            "expected absolute prefix preserved, got {got:?}"
        );
        assert!(got.ends_with(r"lib\foo.ts"), "got {got:?}");
    }
}
