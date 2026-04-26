//! `tsdi` command-line interface.
//!
//! M3: `check` is wired up end-to-end (parse → build graph → validate →
//! render diagnostics via `miette`).
//!
//! M4: `build` lands — same pipeline as `check`, but on success it
//! invokes `tsdi-codegen` and writes one `<component>.tsdi.ts` next to
//! each component's source file. `watch`/`explain` remain stubs (M5+).
//!
//! Diagnostic rendering is intentionally a CLI concern: `tsdi-core` emits
//! structured `Diagnostic` values and never touches disk. The CLI loads
//! source contents on demand and dresses them into `miette::Report`s.

mod diagnostics;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tsdi_codegen::emit_component;
use tsdi_core::graph::{build_and_validate, DependencyGraph, GraphInput};
use tsdi_core::ir::{Binding, ComponentDecl, ModuleDecl};
use tsdi_core::validate::Diagnostic;
use tsdi_parser::symbols::{ProjectGraph, ProjectResolver};

/// Code-generation toolchain for the tsdi TypeScript DI framework.
#[derive(Debug, Parser)]
#[command(name = "tsdi", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Generate component implementations for the configured entries.
    Build {
        /// Path to a `.ts` file containing a `@Component` to emit.
        #[arg(long)]
        entry: PathBuf,
        /// Optional `tsconfig.json` to honor `paths` / `baseUrl`.
        #[arg(long)]
        tsconfig: Option<PathBuf>,
    },
    /// Watch sources and re-emit on change (M5).
    Watch,
    /// Validate the binding graph without emitting code.
    Check {
        /// Path to a `.ts` file containing a `@Component` to validate.
        #[arg(long)]
        entry: PathBuf,
        /// Optional `tsconfig.json` to honor `paths` / `baseUrl`.
        #[arg(long)]
        tsconfig: Option<PathBuf>,
    },
    /// Trace how a key is resolved (M5+).
    Explain {
        /// The key to explain (currently free-form; structured form lands in M5).
        key: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        None => {
            println!(
                "tsdi {} — run `tsdi --help` for usage.",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Some(Command::Check { entry, tsconfig }) => match run_check(entry.as_path(), tsconfig) {
            Ok(_) => ExitCode::SUCCESS,
            Err(CheckError::Diagnostics(n)) => {
                eprintln!("validation failed: {n} diagnostic(s)");
                ExitCode::from(1)
            }
            Err(CheckError::Other(err)) => {
                eprintln!("error: {err:#}");
                ExitCode::from(2)
            }
        },
        Some(Command::Build { entry, tsconfig }) => match run_build(entry.as_path(), tsconfig) {
            Ok(_) => ExitCode::SUCCESS,
            Err(CheckError::Diagnostics(n)) => {
                eprintln!("validation failed: {n} diagnostic(s)");
                ExitCode::from(1)
            }
            Err(CheckError::Other(err)) => {
                eprintln!("error: {err:#}");
                ExitCode::from(2)
            }
        },
        Some(Command::Watch | Command::Explain { .. }) => {
            eprintln!("subcommand not yet implemented; see docs/cli.md");
            ExitCode::from(2)
        }
    }
}

/// Outcome categories of `tsdi check`/`tsdi build`. Distinguishing
/// "validation produced diagnostics" (exit code 1) from "the tool itself
/// errored" (exit code 2) lets CI differentiate genuine pipeline failures
/// from "the user's graph is broken".
enum CheckError {
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
struct ProjectIr {
    modules: Vec<ModuleDecl>,
    components: Vec<ComponentDecl>,
    inject_classes: Vec<Binding>,
}

fn load_project(entry: &Path, tsconfig: Option<PathBuf>) -> Result<ProjectIr, CheckError> {
    let resolver = ProjectResolver::new(tsconfig);
    let project: ProjectGraph =
        ProjectGraph::build_from_entry(entry, &resolver).map_err(anyhow::Error::from)?;

    let mut modules: Vec<ModuleDecl> = Vec::new();
    let mut components: Vec<ComponentDecl> = Vec::new();
    let mut inject_classes: Vec<Binding> = Vec::new();
    for parsed in project.files.values() {
        modules.extend(parsed.modules.iter().cloned());
        components.extend(parsed.components.iter().cloned());
        inject_classes.extend(parsed.inject_classes.iter().cloned());
    }
    Ok(ProjectIr {
        modules,
        components,
        inject_classes,
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

/// Run validation and (on success) emit one `<component>.tsdi.ts` per component.
///
/// Output is co-located with the component's source: a component at
/// `src/coffee/coffee-component.ts` becomes `src/coffee/coffee-component.tsdi.ts`.
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
        let code = emit_component(c, &ir.modules, &ir.inject_classes, version)
            .map_err(anyhow::Error::from)?;
        let out_path = output_path_for(&c.class.module.0)?;
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

/// Map a component's source `.ts` path to its `.tsdi.ts` sibling.
fn output_path_for(component_module: &str) -> Result<PathBuf, CheckError> {
    let p = PathBuf::from(component_module);
    let parent = p
        .parent()
        .ok_or_else(|| anyhow::anyhow!("component path has no parent: {component_module}"))?;
    let stem = p
        .file_stem()
        .ok_or_else(|| anyhow::anyhow!("component path has no file stem: {component_module}"))?;
    let mut out = parent.join(stem);
    out.as_mut_os_string().push(".tsdi.ts");
    Ok(out)
}

/// Result of a successful `tsdi check` run, returned for tests to inspect.
struct CheckSummary {
    #[allow(dead_code)]
    components: usize,
    #[allow(dead_code)]
    bindings: usize,
}

/// Result of a successful `tsdi build` run.
struct BuildSummary {
    #[allow(dead_code)]
    components: usize,
}
