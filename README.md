# tsdi

> **Status:** pre-alpha (M0 scaffolding). Not yet usable.

A code-generation dependency injection framework for TypeScript, modeled after [Dagger 2](https://dagger.dev/). The codegen toolchain is written in Rust and emits plain `.ts` files — there is **no runtime reflection**, **no `reflect-metadata`**, and the dependency graph is fully validated **at build time**.

## Why

The TypeScript DI ecosystem today splits into two camps:

- **Heavy runtime-reflection frameworks** (NestJS, InversifyJS, tsyringe) that depend on `reflect-metadata` and resolve graphs at startup, with errors surfacing at runtime.
- **Compile-time-typed containers** (`typed-inject`, `@wessberg/DI`) that catch mistakes earlier but lack Dagger's higher-level features: scopes, subcomponents, multibindings, and a generated zero-cost component implementation.

`tsdi` aims to fill that gap — Dagger's developer experience, on top of TypeScript, with a Rust toolchain in the spirit of `swc` and `esbuild`.

## How it works

```
   user .ts files (with @Module/@Inject/@Component decorators)
                              |
                              v
          ┌────────────────────────────────────────┐
          │   tsdi (Rust CLI, built on Oxc)        │
          │   parse → IR → graph → validate → emit │
          └────────────────────────────────────────┘
                              |
                              v
        co-located *.tsdi.ts files containing the wired graph
                              |
                              v
                  user's tsc compiles everything
```

User decorators from the `tsdi` npm package are no-op identity functions; all real wiring happens at codegen time.

## Quickstart (target API for v0.1)

```ts
// src/coffee/heater.ts
import { Inject, Singleton } from "tsdi";

@Inject
@Singleton
export class Heater {
  constructor() {}
  on() { console.log("heating"); }
}

// src/coffee/pump.ts
import { Inject } from "tsdi";
import { Heater } from "./heater";

@Inject
export class Pump {
  constructor(private heater: Heater) {}
  pump() { this.heater.on(); }
}

// src/coffee/coffee-component.ts
import { Component, Singleton } from "tsdi";
import { Pump } from "./pump";

@Singleton
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
}
```

```bash
$ tsdi build
# generates src/coffee/coffee-component.tsdi.ts containing DaggerCoffeeShop
```

```ts
// later, in app code
import { createCoffeeShop } from "./coffee/coffee-component.tsdi";
createCoffeeShop().pump().pump();
```

## Repository layout

```
crates/
  tsdi-core/      # IR, dependency graph, validation rules
  tsdi-parser/    # TypeScript parser + decorator extractor (Oxc, M1+)
  tsdi-codegen/   # TS emitter (M4+)
  tsdi-cli/       # `tsdi` binary
packages/
  tsdi/           # runtime: no-op decorator stubs + Token<T>
docs/
  architecture.md, ir.md, codegen.md, validation.md, cli.md
  adr/            # Architecture Decision Records
tests/fixtures/   # golden-file tests for the codegen pipeline (M3+)
examples/         # working sample apps, exercised in CI (M4+)
```

See [`docs/architecture.md`](./docs/architecture.md) for the full pipeline and [`docs/adr/`](./docs/adr/) for the design decisions.

## Roadmap

| Milestone | Scope                                                        |
| --------- | ------------------------------------------------------------ |
| M0        | Workspace scaffolding + CI **(current)**                     |
| M1        | Stage-3 decorator parsing into IR                            |
| M2        | Cross-file symbol resolver (tsconfig-aware)                  |
| M3        | Graph construction + validation (missing/cycle/duplicate)    |
| M4        | First codegen: `@Component` + `@Module` + `@Provides`        |
| M5        | CLI: `build` / `watch` / `check` / `explain`                 |
| **M6**    | `@Inject` ctor + `@Singleton` — **v0.1 release**             |
| M7+       | `@Binds`, `Token<T>`, subcomponents, multibindings, unplugin |

## Building from source

```bash
cargo test --workspace      # Rust unit + snapshot + integration tests
pnpm install
pnpm -r test                # TypeScript runtime tests
pnpm -r build               # Build the runtime package
```

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the full developer workflow and the testing pyramid.

## License

[Apache-2.0](./LICENSE) — chosen to match Dagger and remain GPL-compatible.
