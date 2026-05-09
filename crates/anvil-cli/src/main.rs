//! `anvil` command-line interface.
//!
//! M3: `check` is wired up end-to-end (parse → build graph → validate →
//! render diagnostics via `miette`).
//!
//! M4: `build` lands — same pipeline as `check`, but on success it
//! invokes `anvil-codegen` and writes one `<component>.anvil.ts` next to
//! each component's source file.
//!
//! M5: `explain` traces a key's resolution; `watch` re-emits affected
//! components on filesystem change. All four subcommands accept either
//! `--entry <path>` (single ad-hoc invocation) or `--config <path>`
//! (a `anvil.config.json` or `package.json` whose `anvil` field describes
//! the project's entries). Without either, the CLI tries to discover a
//! config in the current working directory.
//!
//! Diagnostic rendering is intentionally a CLI concern: `anvil-core` emits
//! structured `Diagnostic` values and never touches disk. The CLI loads
//! source contents on demand and dresses them into `miette::Report`s.

mod config;
mod diagnostics;
mod explain;
mod watch;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use anvil_codegen::emit_component;
use anvil_core::graph::{build_and_validate, DependencyGraph, GraphInput};
use anvil_core::ir::{Binding, ComponentDecl, ModuleDecl, SubcomponentDecl};
use anvil_core::validate::Diagnostic;
use anvil_parser::symbols::{ProjectGraph, ProjectResolver};

use crate::config::Config;

/// Code-generation toolchain for the anvil TypeScript DI framework.
#[derive(Debug, Parser)]
#[command(name = "anvil", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Generate component implementations for the configured entries.
    Build(BuildArgs),
    /// Watch sources and re-emit on change.
    Watch(WatchArgs),
    /// Validate the binding graph without emitting code.
    Check(CheckArgs),
    /// Trace how a key is resolved through the binding graph.
    Explain(ExplainArgs),
}

/// Argument shape shared by `build`/`check`/`watch`. Either pass
/// `--entry` (and optionally `--tsconfig`) or `--config <path>`. With
/// neither, the CLI looks for `anvil.config.json` / `package.json` in
/// the current directory.
#[derive(Debug, clap::Args)]
struct ProjectArgs {
    /// Path to a `.ts` file containing a `@Component` to operate on.
    #[arg(long, conflicts_with = "config")]
    entry: Option<PathBuf>,
    /// Optional `tsconfig.json` to honor `paths` / `baseUrl`.
    #[arg(long)]
    tsconfig: Option<PathBuf>,
    /// Path to a `anvil.config.json` (or `package.json` with a `anvil`
    /// field).
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    #[command(flatten)]
    project: ProjectArgs,
}

#[derive(Debug, clap::Args)]
struct CheckArgs {
    #[command(flatten)]
    project: ProjectArgs,
}

#[derive(Debug, clap::Args)]
struct WatchArgs {
    #[command(flatten)]
    project: ProjectArgs,
}

#[derive(Debug, clap::Args)]
struct ExplainArgs {
    /// Key name to trace (e.g. `Pump`). Matches against every binding
    /// in the project's graph; the first hit by lex order wins.
    key: String,
    #[command(flatten)]
    project: ProjectArgs,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        None => {
            println!(
                "anvil {} — run `anvil --help` for usage.",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Some(Command::Check(args)) => run_with_exit(|| {
            let entries = resolve_entries(&args.project)?;
            let mut totals = (0usize, 0usize);
            for entry in &entries {
                let summary = run_check(entry.as_path(), tsconfig_for(&args.project))?;
                totals.0 += summary.components;
                totals.1 += summary.bindings;
            }
            if entries.len() > 1 {
                println!(
                    "ok across {} entry file(s): {} component(s), {} binding(s)",
                    entries.len(),
                    totals.0,
                    totals.1,
                );
            }
            Ok(())
        }),
        Some(Command::Build(args)) => run_with_exit(|| {
            let entries = resolve_entries(&args.project)?;
            let mut total_components = 0usize;
            for entry in &entries {
                let summary = run_build(entry.as_path(), tsconfig_for(&args.project))?;
                total_components += summary.components;
            }
            if entries.len() > 1 {
                println!(
                    "emitted across {} entry file(s): {} component(s)",
                    entries.len(),
                    total_components,
                );
            }
            Ok(())
        }),
        Some(Command::Explain(args)) => run_with_exit(|| {
            let entries = resolve_entries(&args.project)?;
            // Explain is a single-entry inspection tool: error on >1 entry to
            // keep the output unambiguous.
            let entry = match entries.as_slice() {
                [e] => e.clone(),
                _ => {
                    return Err(CheckError::Other(anyhow::anyhow!(
                        "explain requires exactly one entry; got {}",
                        entries.len()
                    )))
                }
            };
            let ir = load_project(entry.as_path(), tsconfig_for(&args.project))?;
            explain::run(&args.key, &ir)?;
            Ok(())
        }),
        Some(Command::Watch(args)) => run_with_exit(|| {
            let entries = resolve_entries(&args.project)?;
            let watch_root = watch_root_for(&args.project, &entries)?;
            watch::run(&watch::Plan {
                entries,
                tsconfig: tsconfig_for(&args.project),
                watch_root,
            })
            .map_err(|e| CheckError::Other(anyhow::Error::from(e)))?;
            Ok(())
        }),
    }
}

/// Convert a `Result<(), CheckError>` produced by a subcommand into an
/// exit code with the standard mapping (0 ok, 1 validation, 2 tooling).
fn run_with_exit(f: impl FnOnce() -> Result<(), CheckError>) -> ExitCode {
    match f() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CheckError::Diagnostics(n)) => {
            eprintln!("validation failed: {n} diagnostic(s)");
            ExitCode::from(1)
        }
        Err(CheckError::Other(err)) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// Resolve the set of entry `.ts` files for the requested operation.
///
/// Precedence:
/// 1. `--entry <path>` (one entry, no glob expansion).
/// 2. `--config <path>` → load + expand `entries` glob.
/// 3. Discover `anvil.config.json` / `package.json` in cwd.
fn resolve_entries(args: &ProjectArgs) -> Result<Vec<PathBuf>, CheckError> {
    if let Some(entry) = &args.entry {
        let abs = std::fs::canonicalize(entry)
            .map_err(|e| anyhow::anyhow!("failed to canonicalize {}: {e}", entry.display()))?;
        return Ok(vec![abs]);
    }
    if let Some(cfg_path) = &args.config {
        let cfg = Config::load(cfg_path).map_err(CheckError::Other)?;
        return cfg.expand_entries().map_err(CheckError::Other);
    }
    let cwd =
        std::env::current_dir().map_err(|e| anyhow::anyhow!("cannot read current dir: {e}"))?;
    if let Some(cfg) = Config::discover(&cwd).map_err(CheckError::Other)? {
        return cfg.expand_entries().map_err(CheckError::Other);
    }
    Err(CheckError::Other(anyhow::anyhow!(
        "no --entry or --config given, and no anvil.config.json / package.json#anvil in {}",
        cwd.display()
    )))
}

/// Pick the tsconfig for the resolver. If `--config` was supplied, its
/// embedded `tsconfig` field wins; otherwise we honor `--tsconfig`.
fn tsconfig_for(args: &ProjectArgs) -> Option<PathBuf> {
    if let Some(cfg_path) = &args.config {
        if let Ok(cfg) = Config::load(cfg_path) {
            return cfg.tsconfig;
        }
    }
    args.tsconfig.clone()
}

/// Pick the directory the watcher should observe. Honors the config's
/// `rootDir` if a config was used; otherwise falls back to the entries'
/// nearest common ancestor (or the first entry's parent).
fn watch_root_for(args: &ProjectArgs, entries: &[PathBuf]) -> Result<PathBuf, CheckError> {
    if let Some(cfg_path) = &args.config {
        return Ok(Config::load(cfg_path).map_err(CheckError::Other)?.root_dir);
    }
    // Discovery path: re-run discover() so root_dir matches the project
    // structure rather than cwd.
    let cwd =
        std::env::current_dir().map_err(|e| anyhow::anyhow!("cannot read current dir: {e}"))?;
    if args.entry.is_none() {
        if let Some(cfg) = Config::discover(&cwd).map_err(CheckError::Other)? {
            return Ok(cfg.root_dir);
        }
    }
    // Fallback: the first entry's parent.
    let first = entries
        .first()
        .ok_or_else(|| anyhow::anyhow!("no entries"))?;
    Ok(first.parent().unwrap_or(first).to_path_buf())
}

/// Outcome categories of `anvil check`/`anvil build`. Distinguishing
/// "validation produced diagnostics" (exit code 1) from "the tool itself
/// errored" (exit code 2) lets CI differentiate genuine pipeline failures
/// from "the user's graph is broken".
pub(crate) enum CheckError {
    /// `n` validation diagnostics were rendered.
    Diagnostics(usize),
    /// The check pipeline itself failed before producing diagnostics.
    Other(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for CheckError {
    fn from(e: E) -> Self {
        CheckError::Other(e.into())
    }
}

/// Aggregated IR for a project, ready to feed to validation/codegen.
pub(crate) struct ProjectIr {
    pub(crate) modules: Vec<ModuleDecl>,
    pub(crate) components: Vec<ComponentDecl>,
    pub(crate) subcomponents: Vec<SubcomponentDecl>,
    pub(crate) inject_classes: Vec<Binding>,
    /// The set of source files the parser walked. Used by watch mode
    /// to recompute the "changed file → affected components" mapping.
    pub(crate) files: Vec<PathBuf>,
}

pub(crate) fn load_project(
    entry: &Path,
    tsconfig: Option<PathBuf>,
) -> Result<ProjectIr, CheckError> {
    let resolver = ProjectResolver::new(tsconfig);
    let project: ProjectGraph =
        ProjectGraph::build_from_entry(entry, &resolver).map_err(anyhow::Error::from)?;

    let mut modules: Vec<ModuleDecl> = Vec::new();
    let mut components: Vec<ComponentDecl> = Vec::new();
    let mut subcomponents: Vec<SubcomponentDecl> = Vec::new();
    let mut inject_classes: Vec<Binding> = Vec::new();
    for parsed in project.files.values() {
        modules.extend(parsed.modules.iter().cloned());
        components.extend(parsed.components.iter().cloned());
        subcomponents.extend(parsed.subcomponents.iter().cloned());
        inject_classes.extend(parsed.inject_classes.iter().cloned());
    }
    let files: Vec<PathBuf> = project.files.keys().map(PathBuf::from).collect();
    Ok(ProjectIr {
        modules,
        components,
        subcomponents,
        inject_classes,
        files,
    })
}

fn run_check(entry: &Path, tsconfig: Option<PathBuf>) -> Result<CheckSummary, CheckError> {
    let ir = load_project(entry, tsconfig)?;

    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut graphs: Vec<DependencyGraph> = Vec::new();
    for c in &ir.components {
        let (g, ds) = build_and_validate(GraphInput {
            component: c,
            modules: &ir.modules,
            inject_classes: &ir.inject_classes,
            subcomponents: &ir.subcomponents,
        });
        graphs.push(g);
        all_diagnostics.extend(ds);
    }

    if !all_diagnostics.is_empty() {
        for d in &all_diagnostics {
            eprintln!("{:?}", diagnostics::render(d));
        }
        return Err(CheckError::Diagnostics(all_diagnostics.len()));
    }

    let total_bindings: usize = graphs.iter().map(DependencyGraph::node_count).sum();
    println!(
        "ok ({} component(s), {} binding(s))",
        ir.components.len(),
        total_bindings,
    );
    Ok(CheckSummary {
        components: ir.components.len(),
        bindings: total_bindings,
    })
}

/// Run validation and (on success) emit one `<component>.anvil.ts` per component.
///
/// Output is co-located with the component's source: a component at
/// `src/coffee/coffee-component.ts` becomes `src/coffee/coffee-component.anvil.ts`.
fn run_build(entry: &Path, tsconfig: Option<PathBuf>) -> Result<BuildSummary, CheckError> {
    let ir = load_project(entry, tsconfig)?;

    // Validate everything first; refuse to write any file if any component
    // produced diagnostics.
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    for c in &ir.components {
        let (_g, ds) = build_and_validate(GraphInput {
            component: c,
            modules: &ir.modules,
            inject_classes: &ir.inject_classes,
            subcomponents: &ir.subcomponents,
        });
        all_diagnostics.extend(ds);
    }
    if !all_diagnostics.is_empty() {
        for d in &all_diagnostics {
            eprintln!("{:?}", diagnostics::render(d));
        }
        return Err(CheckError::Diagnostics(all_diagnostics.len()));
    }

    let version = env!("CARGO_PKG_VERSION");
    let mut written: Vec<PathBuf> = Vec::new();
    for c in &ir.components {
        let code = emit_component(
            c,
            &ir.modules,
            &ir.inject_classes,
            &ir.subcomponents,
            version,
        )
        .map_err(anyhow::Error::from)?;
        let out_path = output_path_for(&c.class.module.abs)?;
        std::fs::write(&out_path, &code)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", out_path.display()))?;
        written.push(out_path);
    }

    println!("emitted {} file(s):", written.len());
    for p in &written {
        println!("  {}", p.display());
    }
    Ok(BuildSummary {
        components: ir.components.len(),
    })
}

/// Map a component's source `.ts` path to its `.anvil.ts` sibling.
fn output_path_for(component_module: &str) -> Result<PathBuf, CheckError> {
    let p = PathBuf::from(component_module);
    let parent = p
        .parent()
        .ok_or_else(|| anyhow::anyhow!("component path has no parent: {component_module}"))?;
    let stem = p
        .file_stem()
        .ok_or_else(|| anyhow::anyhow!("component path has no file stem: {component_module}"))?;
    let mut out = parent.join(stem);
    out.as_mut_os_string().push(".anvil.ts");
    Ok(out)
}

/// Result of a successful `anvil check` run, returned for tests to inspect.
struct CheckSummary {
    components: usize,
    bindings: usize,
}

/// Result of a successful `anvil build` run.
struct BuildSummary {
    components: usize,
}
