//! `anvil-bellows-openapi` — generates `OpenAPI` 3.1 documents from
//! `@Controller` files parsed by `anvil-bellows`.
//!
//! # Pipeline
//!
//! 1. [`anvil_bellows::parse_entry`] walks a directory for `.ts` files and
//!    extracts `@Controller`/`@Get`/`@Tag`/`@Security`/etc. metadata.
//! 2. [`builder::build_openapi`] converts that metadata into an `OpenAPI` 3.1
//!    JSON [`serde_json::Value`].
//! 3. The CLI serialises the document to JSON or YAML.

pub mod builder;
pub mod config;

pub use builder::{build_openapi, BuildDiagnostic};
pub use config::{load_config, OpenApiConfig};
