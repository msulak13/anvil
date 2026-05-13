//! `anvil-bellows-openapi` CLI — generate an `OpenAPI` 3.1 document from
//! `@Controller` source files.

use std::path::PathBuf;
use std::process;

use anvil_bellows::parse_entry;
use anvil_bellows_openapi::{build_openapi, load_config};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "anvil-bellows-openapi",
    about = "Generate an OpenAPI 3.1 document from @Controller source files."
)]
struct Cli {
    /// Directory to scan for `@Controller` files.
    #[arg(long, default_value = "./src")]
    entry: PathBuf,

    /// Output file path.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Output format.
    #[arg(long, default_value = "json", value_parser = ["json", "yaml"])]
    format: String,

    /// Path to a JSON config file (info, servers, securitySchemes).
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let entry = match std::fs::canonicalize(&cli.entry) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("anvil-bellows-openapi: cannot access entry directory: {e}");
            process::exit(1);
        }
    };

    let output = cli.output.unwrap_or_else(|| entry.join("openapi.json"));
    // Canonicalize the output parent dir so relative paths are computed correctly.
    let output = if let Some(parent) = output.parent() {
        if let Ok(canon) = std::fs::canonicalize(parent) {
            canon.join(output.file_name().unwrap())
        } else {
            output
        }
    } else {
        output
    };

    let cfg = match load_config(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("anvil-bellows-openapi: failed to load config: {e}");
            process::exit(1);
        }
    };

    let (files, parse_diags) = match parse_entry(&entry) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("anvil-bellows-openapi: {e}");
            process::exit(1);
        }
    };

    let mut build_diags = Vec::new();
    let doc = build_openapi(&files, &cfg, &mut build_diags);

    let had_errors = !parse_diags.is_empty() || !build_diags.is_empty();
    for d in &parse_diags {
        eprintln!("{}", d.render());
    }
    for d in &build_diags {
        eprintln!("Warning [anvil-bellows-openapi]: {}", d.message);
    }

    let serialized = match cli.format.as_str() {
        "yaml" => match serde_yaml::to_string(&doc) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("anvil-bellows-openapi: YAML serialization error: {e}");
                process::exit(1);
            }
        },
        _ => match serde_json::to_string_pretty(&doc) {
            Ok(s) => s + "\n",
            Err(e) => {
                eprintln!("anvil-bellows-openapi: JSON serialization error: {e}");
                process::exit(1);
            }
        },
    };

    if let Err(e) = std::fs::write(&output, &serialized) {
        eprintln!(
            "anvil-bellows-openapi: failed to write {}: {e}",
            output.display()
        );
        process::exit(1);
    }

    if had_errors {
        process::exit(1);
    }
}
