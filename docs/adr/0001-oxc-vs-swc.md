# 0001 — Oxc as the TypeScript parser

**Status:** Accepted
**Date:** 2026-04-25

## Context

The codegen toolchain needs a Rust-native TypeScript parser that can:

- Parse TC39 Stage-3 decorator syntax
- Preserve type annotations in the AST so we can read parameter types of decorated constructors
- Provide a stable codegen / printer for emitting the generated `.tsdi.ts`
- Run fast enough to drive a sub-200ms watch-mode rebuild on a 100-component project

Two viable options exist:

- **SWC** (`swc_ecma_parser`, `swc_ecma_ast`, `swc_ecma_visit`, `swc_ecma_codegen`) — Apache-2.0, mature, used by Vercel/Next.js, ~13k+ stars. Stage-3 decorators supported via config. Type annotations preserved. Stable codegen.
- **Oxc** (`oxc_parser`, `oxc_ast`, `oxc_codegen`, `oxc_resolver`) — MIT, ~3× faster than SWC per published benchmarks, native Rust, Stage-3 decorators + legacy transformer. Younger but adopted by Rolldown 1.0. Codegen stable; resolver covers tsconfig `paths`.

Biome's parser was considered and rejected: it's not exported as a standalone library and its primary use case is linting, not codegen.

## Decision

**Use Oxc** for parsing, AST manipulation, and codegen, and use `oxc_resolver` for tsconfig-aware module resolution.

## Consequences

### Positive
- Substantially faster cold and incremental builds — important because watch-mode latency is a published target.
- Single-vendor parser + resolver + codegen reduces version-skew risk.
- Aligns with the broader trajectory of the JS-tooling-in-Rust ecosystem (Rolldown, Biome, Oxc-formatter).

### Negative
- Less battle-tested than SWC; some Stage-3 decorator AST corner cases may be missing. Mitigated by an early M1 spike that parses all three v0.1 fixtures and verifies decorator AST shape.
- Smaller community than SWC; fewer Stack Overflow answers when something goes wrong.

## Alternatives considered

- **SWC.** Mature and proven, but slower and not significantly more correct on Stage-3 decorators. Kept as a documented fallback path: if the M1 spike reveals decorator-AST gaps in Oxc, this ADR is superseded by an `0001a-fall-back-to-swc.md`.
- **Roll our own parser.** Categorically rejected — TS is too large and the maintenance cost is incompatible with a small team's bandwidth.
- **Mix Oxc parser with `swc_ecma_codegen`.** Possible in principle, but two AST shapes mean two sets of bugs; the savings are illusory.
