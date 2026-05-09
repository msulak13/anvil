//! TypeScript parser and decorator extractor for the anvil DI framework.
//!
//! Built on Oxc (`oxc_parser`, `oxc_ast`, `oxc_span`). The decision to
//! standardize on Oxc over SWC is recorded in `docs/adr/0001-oxc-vs-swc.md`.
//!
//! Modules:
//!
//! - `imports` — single-file import map (M1).
//! - `decorators` — decorator extractor producing IR (M1).
//! - `symbols` — cross-file resolver (M2).
//!
//! # Example
//!
//! ```
//! use anvil_parser::parse_source;
//!
//! let src = r#"
//!     import { Module, Provides } from "@msulak/anvil";
//!     export class Pump {}
//!     @Module
//!     export class CoffeeModule {
//!         @Provides static providePump(): Pump { return new Pump(); }
//!     }
//! "#;
//! let parsed = parse_source(src, "coffee.ts").unwrap();
//! assert_eq!(parsed.modules.len(), 1);
//! assert_eq!(parsed.modules[0].provides.len(), 1);
//! ```

pub mod decorators;
pub mod imports;
pub mod map_source;
pub mod symbols;

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use thiserror::Error;
use anvil_core::ir::ParsedFile;

use crate::decorators::ExtractError;

/// All ways the parser can fail.
#[derive(Debug, Error)]
pub enum ParseError {
    /// The file could not be read from disk.
    #[error("failed to read {path}: {source}")]
    Io {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying io error.
        #[source]
        source: std::io::Error,
    },
    /// Oxc reported one or more syntax errors.
    ///
    /// The errors are pre-rendered to strings here; M3 will switch to
    /// preserving the original `OxcDiagnostic`s for `miette` rendering.
    #[error("syntax errors in {path}:\n{}", errors.join("\n"))]
    Syntax {
        /// Source path.
        path: String,
        /// Pre-formatted error messages.
        errors: Vec<String>,
    },
    /// The decorator extractor rejected something in the input.
    #[error(transparent)]
    Extract(#[from] ExtractError),
}

/// Result alias used across this crate.
pub type Result<T> = std::result::Result<T, ParseError>;

/// Parse a TypeScript source string and lower it into a [`ParsedFile`].
///
/// `file_path` is informational; it is carried through into the resulting
/// `ParsedFile.path` and into diagnostic messages. The parser does *not*
/// touch the filesystem.
pub fn parse_source(source: &str, file_path: &str) -> Result<ParsedFile> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.errors.is_empty() {
        return Err(ParseError::Syntax {
            path: file_path.to_owned(),
            errors: ret.errors.iter().map(|e| format!("{e:?}")).collect(),
        });
    }

    let imports = imports::build_import_map(&ret.program);
    let local_classes = imports::collect_local_class_names(&ret.program);
    let parsed = decorators::extract(&ret.program, &imports, &local_classes, file_path)?;
    Ok(parsed)
}

/// Read a TypeScript file from disk and lower it into a [`ParsedFile`].
///
/// Equivalent to reading the file and calling [`parse_source`] with the
/// resulting string and the path's display form.
pub fn parse_file(path: &Path) -> Result<ParsedFile> {
    let source = std::fs::read_to_string(path).map_err(|source| ParseError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_source(&source, &path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_file() {
        let parsed = parse_source("", "empty.ts").expect("empty file parses");
        assert!(parsed.modules.is_empty());
        assert!(parsed.components.is_empty());
        assert!(parsed.inject_classes.is_empty());
        assert_eq!(parsed.path, "empty.ts");
    }

    #[test]
    fn syntax_error_is_reported() {
        let err = parse_source("class { ", "broken.ts").expect_err("should fail");
        assert!(matches!(err, ParseError::Syntax { .. }));
    }
}
