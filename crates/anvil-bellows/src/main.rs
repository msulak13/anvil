//! `anvil-bellows` CLI — generate `routes.module.ts` from `@Controller` files.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Generate a `routes.module.ts` module from NestJS-style `@Controller` files.
#[derive(Debug, Parser)]
#[command(name = "anvil-bellows", version, about, long_about = None)]
struct Cli {
    /// Directory to scan for controller files.
    #[arg(long, default_value = "./src")]
    entry: PathBuf,

    /// Output file path. Defaults to `<entry>/routes.module.ts`.
    #[arg(long)]
    output: Option<PathBuf>,

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

    let output = cli
        .output
        .unwrap_or_else(|| entry.join("routes.module.ts"));

    // Canonicalize the output's parent directory so that import specifiers
    // computed against canonicalized source paths are consistent.
    let output = {
        let parent = output.parent().unwrap_or(&entry);
        let canon_parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        canon_parent.join(output.file_name().unwrap_or_default())
    };

    let (files, diagnostics) =
        match anvil_bellows::parse_entry(&entry) {
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
    let code = match anvil_bellows::emit_routes_module(&files, &output, version) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(parent) = output.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "error: cannot create output directory '{}': {e}",
                parent.display()
            );
            return ExitCode::from(2);
        }
    }

    if let Err(e) = std::fs::write(&output, &code) {
        eprintln!("error: cannot write '{}': {e}", output.display());
        return ExitCode::from(2);
    }

    let route_count: usize = files
        .iter()
        .flat_map(|f| &f.controllers)
        .map(|c| c.routes.len())
        .sum();
    println!(
        "anvil-bellows: wrote {} route(s) to {}",
        route_count,
        output.display()
    );

    if had_diagnostics {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
