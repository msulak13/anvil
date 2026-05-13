//! `anvil-bellows` CLI — generate `routes.module.anvil.ts` and
//! `schema-route.module.anvil.ts` from `@Controller` files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

/// Generate routes and `OpenAPI` modules from NestJS-style `@Controller` files.
#[derive(Debug, Parser)]
#[command(name = "anvil-bellows", version, about, long_about = None)]
struct Cli {
    /// Directory to scan for controller files.
    #[arg(long, default_value = "./src")]
    entry: PathBuf,

    /// Output path for the routes module. Defaults to `<entry>/routes.module.anvil.ts`.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Output path for the `OpenAPI` route module.
    /// Defaults to `<entry>/schema-route.module.anvil.ts`.
    #[arg(long)]
    openapi_output: Option<PathBuf>,

    /// `info.title` value in the generated `OpenAPI` spec.
    #[arg(long, default_value = "API")]
    openapi_title: String,

    /// `info.version` value in the generated `OpenAPI` spec.
    #[arg(long, default_value = "0.0.1")]
    openapi_version: String,

    /// Path to `tsconfig.json` (reserved for `--tsc` mode; unused in static mode).
    #[arg(long)]
    tsconfig: Option<PathBuf>,

    /// Enable type-checker mode for resolving non-literal decorator arguments
    /// and schema-typed parameters. (Not yet implemented — reserved for M3.)
    #[arg(long)]
    tsc: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.tsc {
        eprintln!("error: --tsc mode is not yet implemented (planned for M3).");
        return ExitCode::from(2);
    }

    let entry = match std::fs::canonicalize(&cli.entry) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "error: cannot access entry directory '{}': {e}",
                cli.entry.display()
            );
            return ExitCode::from(2);
        }
    };

    let output = resolve_output(cli.output, &entry, "routes.module.anvil.ts");
    let openapi_output = resolve_output(cli.openapi_output, &entry, "schema-route.module.anvil.ts");

    let (files, diagnostics) = match anvil_bellows::parse_entry(&entry) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    // Print diagnostics but continue — non-literal args skip that route, not the file.
    let had_diagnostics = !diagnostics.is_empty();
    for d in &diagnostics {
        eprintln!("{}", d.render());
    }

    if files.is_empty() {
        eprintln!(
            "anvil-bellows: no @Controller classes found under {}",
            entry.display()
        );
        if had_diagnostics {
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }

    let version = env!("CARGO_PKG_VERSION");

    // ── routes module ────────────────────────────────────────────────────────
    let routes_code = match anvil_bellows::emit_routes_module(&files, &output, version) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = write_file(&output, &routes_code) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    // ── OpenAPI route module ─────────────────────────────────────────────────
    let openapi_code = match anvil_bellows::emit_openapi_module(
        &files,
        &openapi_output,
        version,
        &cli.openapi_title,
        &cli.openapi_version,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = write_file(&openapi_output, &openapi_code) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let route_count: usize = files
        .iter()
        .flat_map(|f| &f.controllers)
        .map(|c| c.routes.len())
        .sum();
    println!(
        "anvil-bellows: wrote {} route(s) → {} and {}",
        route_count,
        output.display(),
        openapi_output.display(),
    );

    if had_diagnostics {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Resolve an optional explicit path, or fall back to `<entry>/<default_name>`.
/// Canonicalizes the parent directory so import specifiers computed against
/// canonicalized source paths remain consistent.
fn resolve_output(explicit: Option<PathBuf>, entry: &Path, default_name: &str) -> PathBuf {
    let path = explicit.unwrap_or_else(|| entry.join(default_name));
    let parent = path.parent().unwrap_or(entry);
    let canon_parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    canon_parent.join(path.file_name().unwrap_or_default())
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create directory '{}': {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("cannot write '{}': {e}", path.display()))
}
