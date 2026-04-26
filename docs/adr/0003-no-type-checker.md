# 0003 — No TypeScript type checker in the codegen pipeline

**Status:** Accepted
**Date:** 2026-04-25

## Context

Resolving DI bindings normally requires type information: when the parser sees `private heater: Heater`, it needs to know the *type identity* of `Heater`, not just its identifier. Dagger has this for free because `javac` runs the annotation processor with full type info. We do not have that luxury.

Available options:

1. **Shell out to `tsc`** (or use the TS Compiler API via a Node bridge). Authoritative but slow — every watch-mode iteration would pay tsc startup. The Compiler API is also painful to drive from Rust.
2. **`stc`** — the SWC team's Rust port of `tsc`. **Abandoned** as of 2022; not viable.
3. **Roll our own lightweight type resolver.** Tractable for a constrained subset but balloons in cost as soon as users hit generics, conditional types, or complex unions.
4. **Skip the type checker entirely.** Identify types by `(absolute module path, exported name)` derived from the file's import map. Sufficient for class-typed bindings; non-class types are bound through an explicit `Token<T>` indirection.

## Decision

**Adopt approach (4): no type checker.** A `Key` is a `(ModulePath, name)` pair resolved through the user's import statements. Non-class bindings (interfaces, primitives, configs) are expressed via `Token<T>` (deferred to v0.2 / M7).

The user's own `tsc` validates the **generated** code, which catches mismatches our identity scheme misses (e.g. wrong shape passed to a constructor). This gives us a second line of defense without us running tsc ourselves.

## Consequences

### Positive
- Watch-mode latency target (~200ms) is achievable. tsc startup is comfortably above this on its own.
- Pure Rust pipeline — no Node bridge, no double parsing.
- Forces a clearer user-facing API: explicit `Token<T>` for non-class bindings is more transparent than relying on implicit decorator metadata.

### Negative
- Cannot bind on type aliases, conditional types, or generics that aren't directly classes. v0.1 documents this limitation explicitly.
- Two re-exports of the same class through different barrel files produce **different keys**. Mitigation: the parser follows `export *` chains during M2; users who hit edge cases can fall back to importing the canonical declaration directly.
- Renaming an import (`import { Heater as H } from "./heater"`) is supported, but the bound `Key` always uses the **exported** name, not the local alias. Documented in `ir.md`.

## Alternatives considered

- **(1) Shell out to tsc.** Rejected on latency grounds. Could be revisited as an opt-in `--strict-types` mode in v0.3+ if users hit the limitations of (4) frequently.
- **(2) `stc`.** Not viable — abandoned upstream.
- **(3) Lightweight type resolver.** The slope from "handle simple imports" to "handle generics and conditional types" is steep and well-trodden by TS's own type checker. We'd recreate work that already exists, slowly.

## Revisit

Reopen this decision if any of these hold:

1. Multiple users report hitting limitations that `Token<T>` doesn't paper over (e.g. binding on a non-class type with generics).
2. A maintained Rust TS type-checker emerges (a revived `stc`, a TypeBox-style runtime ergonomics layer, etc.).
3. We can run `tsc --watch` as a long-lived sidecar with sub-100ms incremental responses — startup cost amortized would change the calculus.
