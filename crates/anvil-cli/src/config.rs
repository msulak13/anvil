//! Configuration loading for the `anvil` CLI.
//!
//! Two storage formats are supported:
//!
//! 1. A standalone `anvil.config.json` file.
//! 2. A `anvil` field on `package.json`.
//!
//! Both share the same schema:
//!
//! ```json
//! {
//!   "entries": ["src/**/*-component.ts"],
//!   "tsconfig": "./tsconfig.json",
//!   "outputSuffix": ".anvil.ts",
//!   "rootDir": "src"
//! }
//! ```
//!
//! Paths are interpreted relative to the file the config was loaded from
//! and are canonicalized (when the target exists) when [`Config::load`]
//! returns.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Resolved configuration for a `anvil` invocation.
///
/// All paths are absolute (when their targets exist on disk). Entry
/// patterns remain in their original glob form — call
/// [`Config::expand_entries`] to materialize them into concrete files.
#[derive(Debug, Clone)]
pub struct Config {
    /// Glob patterns identifying `@Component` source files. Resolved
    /// relative to `root_dir` (or the config's directory when
    /// `root_dir` is unset).
    pub entries: Vec<String>,
    /// Optional plugins to load (WASM or script).
    pub plugins: Vec<String>,
    /// Optional `tsconfig.json` path used by the resolver to honor
    /// `paths` / `baseUrl`.
    pub tsconfig: Option<PathBuf>,
    /// File suffix appended to a component's stem to compute the
    /// emitted file name. Defaults to `.anvil.ts`. Honored in M5+.
    #[allow(dead_code)]
    pub output_suffix: String,
    /// Directory used as the base for `entries` glob expansion and as
    /// the root of any watch session. Defaults to the directory the
    /// config was loaded from.
    pub root_dir: PathBuf,
    /// Absolute path to the file the config was loaded from. Used by
    /// diagnostics so the user knows which config produced an error.
    #[allow(dead_code)]
    pub source: PathBuf,
}

/// Raw JSON shape of a `anvil` config block. Defaults are applied in
/// [`Config::from_raw`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConfig {
    entries: Option<Vec<String>>,
    plugins: Option<Vec<String>>,
    tsconfig: Option<String>,
    output_suffix: Option<String>,
    root_dir: Option<String>,
}

/// Wrapper for `package.json` so we can pluck out the optional `anvil` field.
#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    anvil: Option<RawConfig>,
}

impl Config {
    /// Load a config from a file. The file may be either a
    /// `anvil.config.json` (raw config schema) or a `package.json`
    /// (read its `anvil` field).
    ///
    /// # Errors
    /// Returns an error if the file does not exist, cannot be read,
    /// fails JSON parsing, or — for `package.json` — has no `anvil`
    /// field.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        let dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", path.display()))?;
        let raw = if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "package.json")
        {
            let pkg: PackageJson = serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("failed to parse {} as package.json: {e}", path.display())
            })?;
            pkg.anvil
                .ok_or_else(|| anyhow::anyhow!("{} has no `anvil` field", path.display()))?
        } else {
            serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("failed to parse {} as anvil config: {e}", path.display())
            })?
        };
        Self::from_raw(raw, dir, path)
    }

    /// Try to find a config relative to `start`. Looks for
    /// `anvil.config.json` first, then `package.json` (only if it has a
    /// `anvil` field). Returns `Ok(None)` if neither matched.
    ///
    /// # Errors
    /// Surfaces I/O or parse errors from the matched candidate.
    pub fn discover(start: &Path) -> anyhow::Result<Option<Self>> {
        let cfg = start.join("anvil.config.json");
        if cfg.is_file() {
            return Ok(Some(Self::load(&cfg)?));
        }
        let pkg = start.join("package.json");
        if pkg.is_file() {
            // Probe to see whether `anvil` is present without surfacing
            // unrelated package.json parse errors.
            let text = std::fs::read_to_string(&pkg)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", pkg.display()))?;
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", pkg.display()))?;
            if parsed.get("anvil").is_some() {
                return Ok(Some(Self::load(&pkg)?));
            }
        }
        Ok(None)
    }

    /// Expand `entries` (relative-glob patterns) into a list of
    /// canonical, absolute `.ts` file paths.
    ///
    /// # Errors
    /// Returns an error if any pattern fails to compile or if an
    /// expanded path cannot be canonicalized.
    pub fn expand_entries(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut out: Vec<PathBuf> = Vec::new();
        for pat in &self.entries {
            // Globs are anchored to root_dir.
            let abs_pat = self.root_dir.join(pat);
            let pat_str = abs_pat.to_string_lossy().into_owned();
            let matches =
                glob::glob(&pat_str).map_err(|e| anyhow::anyhow!("invalid glob `{pat}`: {e}"))?;
            for m in matches {
                let p = m.map_err(|e| anyhow::anyhow!("glob error: {e}"))?;
                let canon = std::fs::canonicalize(&p)
                    .map_err(|e| anyhow::anyhow!("failed to canonicalize {}: {e}", p.display()))?;
                if !out.contains(&canon) {
                    out.push(canon);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn from_raw(raw: RawConfig, dir: &Path, source: &Path) -> anyhow::Result<Self> {
        let dir = std::fs::canonicalize(dir)
            .map_err(|e| anyhow::anyhow!("failed to canonicalize {}: {e}", dir.display()))?;
        let entries = raw.entries.unwrap_or_default();
        if entries.is_empty() {
            return Err(anyhow::anyhow!(
                "config at {} has no `entries`",
                source.display()
            ));
        }
        let plugins = raw.plugins.unwrap_or_default();
        let tsconfig = raw
            .tsconfig
            .map(|t| dir.join(t))
            .map(|p| std::fs::canonicalize(&p).unwrap_or(p));
        let output_suffix = raw.output_suffix.unwrap_or_else(|| ".anvil.ts".to_owned());
        let root_dir = raw
            .root_dir
            .map(|r| dir.join(r))
            .map_or_else(|| dir.clone(), |p| std::fs::canonicalize(&p).unwrap_or(p));
        Ok(Self {
            entries,
            plugins,
            tsconfig,
            output_suffix,
            root_dir,
            source: source.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn loads_standalone_config_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("src/coffee.ts"), "// component");
        write(
            &root.join("anvil.config.json"),
            r#"{ "entries": ["src/*.ts"], "rootDir": ".", "plugins": ["my-plugin.wasm"] }"#,
        );

        let cfg = Config::load(&root.join("anvil.config.json")).unwrap();
        let entries = cfg.expand_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].ends_with("coffee.ts"));
        assert_eq!(cfg.output_suffix, ".anvil.ts");
        assert_eq!(cfg.plugins, vec!["my-plugin.wasm"]);
    }

    #[test]
    fn loads_from_package_json_anvil_field() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("src/comp.ts"), "// component");
        write(
            &root.join("package.json"),
            r#"{ "name": "x", "anvil": { "entries": ["src/*.ts"] } }"#,
        );

        let cfg = Config::load(&root.join("package.json")).unwrap();
        assert_eq!(cfg.entries, vec!["src/*.ts".to_owned()]);
        assert_eq!(cfg.expand_entries().unwrap().len(), 1);
    }

    #[test]
    fn discover_prefers_anvil_config_over_package_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("src/comp.ts"), "// component");
        write(
            &root.join("anvil.config.json"),
            r#"{ "entries": ["src/*.ts"] }"#,
        );
        write(
            &root.join("package.json"),
            r#"{ "name": "x", "anvil": { "entries": ["NEVER"] } }"#,
        );
        let cfg = Config::discover(root).unwrap().unwrap();
        assert_eq!(cfg.entries, vec!["src/*.ts".to_owned()]);
    }

    #[test]
    fn discover_returns_none_when_no_config() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("package.json"), r#"{ "name": "x" }"#);
        assert!(Config::discover(root).unwrap().is_none());
    }

    #[test]
    fn empty_entries_list_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("anvil.config.json"), r#"{ "entries": [] }"#);
        let err = Config::load(&root.join("anvil.config.json")).unwrap_err();
        assert!(format!("{err}").contains("no `entries`"));
    }
}
