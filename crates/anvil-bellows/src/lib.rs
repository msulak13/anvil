//! anvil-bellows codegen library — parses `@Controller` files and emits `routes.module.ts`.
//!
//! # Pipeline
//!
//! 1. `parser::parse_entry` walks a directory for `.ts` files and extracts
//!    `@Controller`/`@Get`/`@Post`/etc. metadata using Oxc.
//! 2. `codegen::emit_routes_module` takes that metadata and produces a
//!    `routes.module.ts` string, run through `oxc_codegen` for canonical
//!    formatting.

pub mod codegen;
pub mod parser;

pub use codegen::{emit_openapi_module, emit_routes_module, EmitError};
pub use parser::{
    parse_entry, AuthRef, CodecRef, Controller, ControllerFile, ExtraResponse, HttpMethod,
    ImportOrigin, MiddlewareRef, ParamKind, ParseDiagnostic, ReturnKind, Route, SchemaRef,
    TypedParam,
};
