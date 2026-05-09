//! Core IR, dependency graph, and validation rules for the anvil TypeScript DI framework.
//!
//! This crate is intentionally free of any TypeScript parsing or code emission. It exposes:
//!
//! - [`ir`]: the intermediate representation (`Key`, `Binding`, `Provider`, `Scope`,
//!   `ModuleDecl`, `ComponentDecl`) that the parser produces and the codegen consumes.
//! - [`graph`]: a [`petgraph`]-backed dependency graph builder.
//! - [`validate`]: rules that catch missing bindings, cycles, duplicate bindings, and
//!   scope mismatches before code is emitted.
//!
//! See `docs/architecture.md` and `docs/ir.md` in the repository root for the full design.

pub mod graph;
pub mod ir;
pub mod validate;

/// Crate version, surfaced in generated-file banners so users can map an emitted
/// `.anvil.ts` back to the toolchain that produced it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_matches_cargo_pkg() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
