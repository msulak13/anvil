# 0002 — Stage-3 decorators only (no `experimentalDecorators`)

**Status:** Accepted
**Date:** 2026-04-25

## Context

TypeScript supports two distinct decorator implementations:

- **Legacy** — enabled by `experimentalDecorators: true`. Used by NestJS, TypeORM, InversifyJS, tsyringe, Angular. Often paired with `emitDecoratorMetadata` and `reflect-metadata`.
- **TC39 Stage-3** — native to TypeScript 5.0+, no flag required, follows the JavaScript proposal that's expected to ship as Stage 4 in the near term. Different runtime semantics, different AST shape.

User code using one syntax cannot consume libraries written against the other without contortions. Supporting both doubles parser fixtures, codegen tests, and edge-case handling on every milestone — particularly painful when the AST shapes diverge (legacy decorators are member-level expressions; Stage-3 are first-class with context objects).

## Decision

**`tsdi` supports Stage-3 decorators only.** Sample code, fixtures, examples, and the runtime `tsdi` package's decorators all assume Stage-3 semantics. `tsconfig.base.json` explicitly sets `experimentalDecorators: false`. The parser will reject (with a clear diagnostic) any input file that uses legacy decorator shapes.

## Consequences

### Positive
- Smaller test matrix: one decorator shape, one parser path, one codegen path.
- Future-proof: TC39 Stage-3 is the long-term direction; legacy decorators are explicitly deprecated by TS leadership.
- Aligns with TypeScript 5.0+ defaults — users on modern toolchains incur zero migration cost.
- Generated code can rely on the well-defined `ClassDecoratorContext` / `ClassMethodDecoratorContext` types, simplifying any runtime introspection if added later.

### Negative
- Excludes users on existing legacy-decorator codebases (NestJS, older Angular). They cannot adopt `tsdi` without migrating their decorators.
- The user-facing API will look different from familiar Dagger-style examples that rely on legacy decorator-metadata patterns.

## Alternatives considered

- **Legacy only.** Maximizes ecosystem compatibility today but bets against the future direction of the language. Would also force generated code into older patterns.
- **Both.** Explored and rejected — see Context. Cost too high for a small team and obscures the spec.
- **Defer the decision.** Untenable: the parser, runtime, and codegen all need to commit to a syntax before M1 makes meaningful progress.

## Revisit

Reopen this decision if either condition holds:

1. A significant fraction of user-reported demand explicitly cites legacy decorator support (e.g. NestJS interop).
2. TC39 advances Stage-3 decorators to Stage 4 *and* a clean compat shim for legacy decorators emerges in the ecosystem that we could adopt without doubling our test matrix.
