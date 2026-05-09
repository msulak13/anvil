//! WASM bindings for the anvil codegen pipeline (M13).
//!
//! Exposes a single high-level entry point — [`compile`] — that
//! takes a self-contained input bundle (entry path, in-memory file
//! map, optional tsconfig paths aliases) and returns the emitted
//! `*.anvil.ts` strings plus any structured diagnostics. No
//! filesystem access, no `oxc_resolver`, no spawn cost.
//!
//! Same pipeline as the native CLI, just sourced from a `HashMap`
//! and exposed through `wasm-bindgen` JS interop:
//!
//! 1. `anvil_parser::ProjectGraph::build_from_map` walks the entry's
//!    transitive imports against the supplied [`FileMap`].
//! 2. For each `@Component` in the graph, `anvil_codegen::emit_component`
//!    produces the `.anvil.ts` string (parser-validated through
//!    `oxc_parser` + canonicalized through `oxc_codegen`, same as
//!    native).
//! 3. Diagnostics from the graph layer are converted into
//!    JS-friendly DTOs (no `miette` dressing — the host renders them
//!    however it wants).
//!
//! The crate also exposes the same logic as a regular Rust function
//! ([`compile_native`]) so test code can exercise the M13 path
//! without pulling in a wasm runtime.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wasm_bindgen::prelude::*;

use anvil_core::graph::{build_and_validate, GraphInput};
use anvil_core::ir::{Binding, ComponentDecl, ModuleDecl, ParsedFile, SubcomponentDecl};
use anvil_core::validate::Diagnostic;
use anvil_parser::map_source::{FileMap, MapResolver, PathAlias};
use anvil_parser::symbols::{ProjectGraph, SymbolError};

// ───────── Public DTOs (serde-friendly, JS-shaped) ────────────────

/// Input to [`compile`] / [`compile_native`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileInput {
    /// Absolute path of the `@Component` entry file. Must be present
    /// as a key in [`Self::files`].
    pub entry_path: String,
    /// File contents keyed by absolute path. Must include the entry
    /// and every transitively-reachable source file.
    pub files: HashMap<String, String>,
    /// Optional tsconfig `paths` aliases. The host is expected to
    /// pre-parse the user's tsconfig (which it has to read for its
    /// own purposes anyway) and pass through whatever entries
    /// `compilerOptions.paths` declares.
    #[serde(default)]
    pub aliases: Vec<AliasDto>,
    /// Version string surfaced into the generated dagger's banner
    /// comment. Lets users correlate emitted output with the
    /// `anvil-codegen-wasm` package version that produced it.
    pub version: String,
}

/// One tsconfig `paths` alias, JS-shaped.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasDto {
    /// The alias pattern (e.g. `"@/*"`).
    pub pattern: String,
    /// The first matching target template (e.g. `"src/*"`).
    pub target: String,
    /// Absolute directory the relative target paths resolve against
    /// (typically the tsconfig's own dir, or its `baseUrl`).
    pub base_dir: String,
}

/// Output of [`compile`] / [`compile_native`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileOutput {
    /// `*.anvil.ts` files emitted, one per `@Component`. The host
    /// either writes these to disk or feeds them straight into the
    /// bundler's transform output.
    pub emitted_files: Vec<EmittedFile>,
    /// Structured diagnostics produced during graph validation.
    /// Empty for a successful compile; non-empty means **at least**
    /// one component failed validation. The host is responsible for
    /// surfacing these through whatever channel it has (Vite overlay,
    /// stderr, etc.).
    pub diagnostics: Vec<DiagnosticDto>,
}

/// A single emitted file.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmittedFile {
    /// Absolute path the dagger should be written at (or the key
    /// the host should hand to the bundler's virtual module graph).
    pub path: String,
    /// The emitted source. Standard `.ts` — there's no special
    /// post-processing the host needs to do.
    pub contents: String,
}

/// JS-friendly view of [`anvil_core::validate::Diagnostic`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticDto {
    /// Stable diagnostic code (e.g. `"anvil::missing_binding"`).
    pub code: String,
    /// Human-readable one-line summary.
    pub summary: String,
    /// The primary source location.
    pub primary: SpanLabelDto,
    /// Additional related locations (cycle members, duplicate
    /// declarations, etc.).
    pub related: Vec<SpanLabelDto>,
}

/// JS-friendly view of [`anvil_core::validate::Label`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanLabelDto {
    /// Absolute source path.
    pub path: String,
    /// Inclusive byte offset of the first character.
    pub start: u32,
    /// Exclusive byte offset just past the last character.
    pub end: u32,
    /// Human-readable note rendered next to the source span.
    pub message: String,
}

// ───────── Errors ────────────────────────────────────────────────

/// Top-level errors from the WASM entry point. Wraps everything that
/// can go wrong before we even get to graph validation (parser
/// errors, missing files, malformed input). Validation diagnostics
/// flow through [`CompileOutput::diagnostics`] instead — they're not
/// fatal at the API level.
#[derive(Debug, Error)]
pub enum CompileError {
    /// The supplied `entryPath` doesn't exist in the file map, or a
    /// transitively-imported file is missing.
    #[error("symbol resolution failed: {0}")]
    Symbol(#[from] SymbolError),
    /// The host passed JSON that didn't match `CompileInput`.
    #[error("malformed input: {0}")]
    Input(String),
}

// ───────── Public entry points ───────────────────────────────────

/// Run the full codegen pipeline against an in-memory file map.
///
/// This is the function you'd call from Rust test code or from
/// another Rust crate that wants to embed anvil without spawning a
/// process. The wasm-bindgen entry point [`compile`] thinly wraps
/// this with JS interop.
///
/// # Errors
///
/// See [`CompileError`].
pub fn compile_native(input: CompileInput) -> Result<CompileOutput, CompileError> {
    // wasm32-unknown-unknown's `std::path` uses Unix-style semantics
    // — backslashes are *part of the filename*, not separators. Hosts
    // running on Windows would otherwise hand us paths like
    // `C:\Users\foo\bar.ts` that parse as a single Normal component,
    // breaking `Path::parent()` and every relative-import resolution.
    // Normalize both the entry and every file-map key to forward
    // slashes up front. The output paths follow suit; hosts running
    // on Windows can convert back to backslashes when writing the
    // emitted files to disk.
    let normalize_sep = |s: &str| -> String { s.replace('\\', "/") };

    // Build the file map.
    let files = FileMap::from_pairs(
        input
            .files
            .into_iter()
            .map(|(k, v)| (PathBuf::from(normalize_sep(&k)), v)),
    );

    // Build the resolver with whatever aliases the host supplied.
    let aliases: Vec<PathAlias> = input
        .aliases
        .into_iter()
        .map(|a| PathAlias {
            pattern: a.pattern,
            target: a.target,
            base_dir: PathBuf::from(normalize_sep(&a.base_dir)),
        })
        .collect();
    let resolver = MapResolver::with_aliases(aliases);

    // Walk the project graph from the entry.
    let entry_pb = PathBuf::from(normalize_sep(&input.entry_path));
    let graph = ProjectGraph::build_from_map(&entry_pb, &files, &resolver)?;

    // Aggregate per-graph project-wide bindings exactly the way the
    // native CLI does: every @Module, every @Component, every
    // @Subcomponent, every @Inject self-binding from every parsed
    // file becomes input to the validator/emitter.
    let mut all_modules: Vec<ModuleDecl> = Vec::new();
    let mut all_components: Vec<ComponentDecl> = Vec::new();
    let mut all_subcomponents: Vec<SubcomponentDecl> = Vec::new();
    let mut all_inject_classes: Vec<Binding> = Vec::new();
    for parsed in graph.files.values() {
        gather(
            parsed,
            &mut all_modules,
            &mut all_components,
            &mut all_subcomponents,
            &mut all_inject_classes,
        );
    }

    // For each @Component, run validation + emit.
    let mut emitted_files: Vec<EmittedFile> = Vec::new();
    let mut diagnostics: Vec<DiagnosticDto> = Vec::new();
    for component in &all_components {
        let (_g, diags) = build_and_validate(GraphInput {
            component,
            modules: &all_modules,
            inject_classes: &all_inject_classes,
            subcomponents: &all_subcomponents,
        });
        if !diags.is_empty() {
            for d in diags {
                diagnostics.push(diagnostic_to_dto(&d));
            }
            // Skip emission for this component; downstream consumers
            // only get a `.anvil.ts` for the components that
            // validated cleanly.
            continue;
        }
        match anvil_codegen::emit_component(
            component,
            &all_modules,
            &all_inject_classes,
            &all_subcomponents,
            &input.version,
        ) {
            Ok(contents) => {
                let abs = PathBuf::from(&component.class.module.abs);
                let dagger_path = abs.with_extension("").as_os_str().to_owned();
                let mut path_buf = PathBuf::from(dagger_path);
                // `with_extension` strips `.ts` cleanly; we now want
                // `<stem>.anvil.ts`. Append directly.
                let mut final_path = path_buf.as_os_str().to_owned();
                final_path.push(".anvil.ts");
                path_buf = PathBuf::from(final_path);
                emitted_files.push(EmittedFile {
                    path: path_buf.to_string_lossy().into_owned(),
                    contents,
                });
            }
            Err(anvil_codegen::EmitError::Invalid(diags)) => {
                for d in diags {
                    diagnostics.push(diagnostic_to_dto(&d));
                }
            }
            Err(other) => {
                // Non-validation emit failures are programming bugs
                // (bad component path, emitter syntax error). Surface
                // them as a synthetic diagnostic so the host can still
                // proceed with other components.
                diagnostics.push(synthesize_emit_diagnostic(
                    &component.class.module.abs,
                    &other,
                ));
            }
        }
    }

    Ok(CompileOutput {
        emitted_files,
        diagnostics,
    })
}

/// `wasm-bindgen` entry point. Accepts a JS object matching
/// [`CompileInput`], returns a JS object matching [`CompileOutput`].
/// Errors thrown into JS land map to the host's normal exception
/// handling — `@msulak/anvil-unplugin` translates these into bundler-side
/// diagnostic surfaces.
#[wasm_bindgen]
pub fn compile(input_js: JsValue) -> Result<JsValue, JsValue> {
    let input: CompileInput = serde_wasm_bindgen::from_value(input_js)
        .map_err(|e| JsValue::from_str(&format!("malformed input: {e}")))?;
    let output = compile_native(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&output).map_err(JsValue::from)
}

// ───────── Internals ─────────────────────────────────────────────

fn gather(
    parsed: &ParsedFile,
    all_modules: &mut Vec<ModuleDecl>,
    all_components: &mut Vec<ComponentDecl>,
    all_subcomponents: &mut Vec<SubcomponentDecl>,
    all_inject_classes: &mut Vec<Binding>,
) {
    all_modules.extend(parsed.modules.iter().cloned());
    all_components.extend(parsed.components.iter().cloned());
    all_subcomponents.extend(parsed.subcomponents.iter().cloned());
    all_inject_classes.extend(parsed.inject_classes.iter().cloned());
}

fn diagnostic_to_dto(d: &Diagnostic) -> DiagnosticDto {
    DiagnosticDto {
        code: d.code().to_owned(),
        summary: d.summary(),
        primary: SpanLabelDto {
            path: d.primary.span.path.clone(),
            start: d.primary.span.start,
            end: d.primary.span.end,
            message: d.primary.message.clone(),
        },
        related: d
            .related
            .iter()
            .map(|l| SpanLabelDto {
                path: l.span.path.clone(),
                start: l.span.start,
                end: l.span.end,
                message: l.message.clone(),
            })
            .collect(),
    }
}

fn synthesize_emit_diagnostic(
    component_path: &str,
    err: &anvil_codegen::EmitError,
) -> DiagnosticDto {
    DiagnosticDto {
        code: "anvil::emit_failed".to_owned(),
        summary: format!("emitter failed for {component_path}: {err}"),
        primary: SpanLabelDto {
            path: component_path.to_owned(),
            start: 0,
            end: 0,
            message: err.to_string(),
        },
        related: Vec::new(),
    }
}

// ───────── Native-only tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn coffee_project() -> CompileInput {
        let mut files = HashMap::new();
        files.insert(
            "/proj/node_modules/@msulak/anvil/index.d.ts".to_owned(),
            "export const Inject: any; export const Module: any; \
             export const Provides: any; export const Component: any; \
             export const Singleton: any;"
                .to_owned(),
        );
        files.insert(
            "/proj/src/heater.ts".to_owned(),
            r#"
                import { Inject, Singleton } from "@msulak/anvil";
                @Inject @Singleton
                export class Heater { constructor() {} }
            "#
            .to_owned(),
        );
        files.insert(
            "/proj/src/pump.ts".to_owned(),
            r#"
                import { Inject } from "@msulak/anvil";
                import { Heater } from "./heater";
                @Inject
                export class Pump { constructor(private heater: Heater) {} }
            "#
            .to_owned(),
        );
        files.insert(
            "/proj/src/app-component.ts".to_owned(),
            r#"
                import { Component, Singleton } from "@msulak/anvil";
                import { Pump } from "./pump";
                @Singleton
                @Component({ modules: [] })
                export abstract class App { abstract pump(): Pump; }
            "#
            .to_owned(),
        );
        CompileInput {
            entry_path: "/proj/src/app-component.ts".to_owned(),
            files,
            aliases: Vec::new(),
            version: "0.0.1-wasm".to_owned(),
        }
    }

    #[test]
    fn compiles_a_simple_project_to_a_dagger() {
        let out = compile_native(coffee_project()).expect("compiles");
        assert_eq!(
            out.diagnostics.len(),
            0,
            "diagnostics: {:?}",
            out.diagnostics
        );
        assert_eq!(out.emitted_files.len(), 1);
        let dagger = &out.emitted_files[0];
        assert!(dagger.path.ends_with("app-component.anvil.ts"));
        assert!(dagger.contents.contains("DaggerApp"));
        assert!(dagger.contents.contains("createApp"));
        assert!(dagger.contents.contains("0.0.1-wasm"));
    }

    #[test]
    #[cfg(windows)]
    fn compiles_a_windows_absolute_path_project() {
        // Regression: M13 had a bug where Windows absolute paths
        // round-tripped through PathBuf::push lost their drive prefix,
        // making `path.parent()` return an empty PathBuf and breaking
        // every `./relative` import.
        let mut files = HashMap::new();
        let stub = "export const Inject: any; export const Component: any; \
                    export const Singleton: any;";
        files.insert(
            r"C:\proj\node_modules\@msulak\anvil\index.d.ts".to_owned(),
            stub.to_owned(),
        );
        files.insert(
            r"C:\proj\src\heater.ts".to_owned(),
            r#"import { Inject } from "@msulak/anvil"; @Inject export class Heater {}"#.to_owned(),
        );
        files.insert(
            r"C:\proj\src\app.ts".to_owned(),
            r#"
                import { Component } from "@msulak/anvil";
                import { Heater } from "./heater";
                @Component({ modules: [] })
                export abstract class App { abstract heater(): Heater; }
            "#
            .to_owned(),
        );
        let out = compile_native(CompileInput {
            entry_path: r"C:\proj\src\app.ts".to_owned(),
            files,
            aliases: Vec::new(),
            version: "0.0.1".to_owned(),
        })
        .expect("compiles");
        assert_eq!(
            out.diagnostics.len(),
            0,
            "diagnostics: {:?}",
            out.diagnostics
        );
        assert_eq!(out.emitted_files.len(), 1);
    }

    #[test]
    fn surfaces_validation_diagnostics_through_the_dto() {
        // Pump depends on Heater, but Heater is not @Inject anywhere
        // (we strip the @Inject from heater.ts).
        let mut input = coffee_project();
        input.files.insert(
            "/proj/src/heater.ts".to_owned(),
            "export class Heater { constructor() {} }".to_owned(),
        );
        let out = compile_native(input).expect("compile_native succeeds (diagnostics non-fatal)");
        assert!(
            !out.diagnostics.is_empty(),
            "expected at least one diagnostic"
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == "anvil::missing_binding"),
            "expected a missing-binding diagnostic; got: {:?}",
            out.diagnostics,
        );
        // No file emitted for the failing component.
        assert!(out.emitted_files.is_empty());
    }
}
