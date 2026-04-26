/**
 * Runtime stubs and types for the `tsdi` compile-time dependency injection framework.
 *
 * Decorators exported from this package are intentionally no-ops at runtime —
 * all real work happens at codegen time, performed by the `tsdi` Rust CLI which
 * reads decorated source and emits `*.tsdi.ts` files containing the wired graph.
 *
 * See `docs/architecture.md` for the full pipeline.
 */

export * from "./decorators.js";
export * from "./token.js";
