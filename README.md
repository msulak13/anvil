# anvil

> **Status:** pre-alpha (v0.0.x). Core codegen is functional; API is unstable.

A code-generation dependency injection framework for TypeScript, modeled after [Dagger 2](https://dagger.dev/). The codegen toolchain is written in Rust and emits plain `.ts` files — there is **no runtime reflection**, **no `reflect-metadata`**, and the dependency graph is fully validated **at build time**.

## Why

The TypeScript DI ecosystem today splits into two camps:

- **Heavy runtime-reflection frameworks** (NestJS, InversifyJS, tsyringe) that depend on `reflect-metadata` and resolve graphs at startup, with errors surfacing at runtime.
- **Compile-time-typed containers** (`typed-inject`, `@wessberg/DI`) that catch mistakes earlier but lack Dagger's higher-level features: scopes, subcomponents, multibindings, and a generated zero-cost component implementation.

`anvil` aims to fill that gap — Dagger's developer experience, on top of TypeScript, with a Rust toolchain in the spirit of `swc` and `esbuild`.

## How it works

```
   user .ts files (with @Module/@Inject/@Component decorators)
                             |
                             v
         ┌────────────────────────────────────────┐
         │   anvil (Rust CLI, built on Oxc)        │
         │   parse → IR → graph → validate → emit │
         └────────────────────────────────────────┘
                             |
                             v
       co-located *.anvil.ts files containing the wired graph
                             |
                             v
                 user's tsc compiles everything
```

User decorators from the `@anvil-di/anvil` npm package are no-op identity functions; all real wiring happens at codegen time.

## Quickstart

```bash
npm install -D @anvil-di/anvil-cli @anvil-di/anvil
```

```ts
// src/coffee/heater.ts
import { Inject, Singleton } from "@anvil-di/anvil";

@Inject
@Singleton
export class Heater {
  on() { console.log("heating"); }
}

// src/coffee/pump.ts
import { Inject } from "@anvil-di/anvil";
import { Heater } from "./heater";

@Inject
export class Pump {
  constructor(private heater: Heater) {}
  pump() { this.heater.on(); }
}

// src/coffee/coffee-component.ts
import { Component, Singleton } from "@anvil-di/anvil";
import { Pump } from "./pump";

@Singleton
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
}
```

```bash
$ anvil build
# generates src/coffee/coffee-component.anvil.ts containing DaggerCoffeeShop
```

```ts
// app.ts
import { createCoffeeShop } from "./coffee/coffee-component.anvil";
createCoffeeShop().pump().pump();
```

### Bundler plugin (Vite / Rollup / webpack / esbuild / Rspack)

```bash
npm install -D @anvil-di/anvil-unplugin
```

```ts
// vite.config.ts
import { anvilPlugin } from "@anvil-di/anvil-unplugin/vite";
export default { plugins: [anvilPlugin()] };
```

Pass `mode: "wasm"` to run codegen in-process via `@anvil-di/anvil-codegen-wasm` without a native binary.

## Supported features

| Feature | Decorator(s) |
| ------- | ------------ |
| Constructor injection | `@Inject` |
| Singleton scoping | `@Singleton` |
| Module factory methods | `@Module` / `@Provides` |
| Interface aliasing | `@Binds` |
| Subcomponents | `@Subcomponent` |
| Set multibindings | `@IntoSet` |
| Async providers | `async @Provides` returning `Promise<T>` |
| Subcomponent factory params | method params on `@Subcomponent` factory |

### Bellows — NestJS-style controller codegen

`@anvil-di/bellows` adds a companion pipeline that parses `@Controller` / `@Get` / `@Post` / … decorator files and emits a typed `routes.module.ts` with `safeParse` validation prologues, plus an optional OpenAPI spec.

```bash
npm install -D @anvil-di/bellows-cli @anvil-di/bellows
anvil-bellows --entry src/controllers
```

## Repository layout

```
crates/
  anvil-core/              # IR, dependency graph, validation rules
  anvil-parser/            # TypeScript parser + decorator extractor (Oxc)
  anvil-codegen/           # TS emitter
  anvil-codegen-wasm/      # WASM build of codegen (for unplugin wasm mode)
  anvil-cli/               # `anvil` binary (build/check/watch/explain)
  anvil-bellows/           # `anvil-bellows` binary — controller route emitter
  anvil-bellows-openapi/   # `anvil-bellows-openapi` binary — OpenAPI spec emitter
packages/
  anvil/                   # @anvil-di/anvil — no-op decorator stubs + Token<T>
  anvil-unplugin/          # @anvil-di/anvil-unplugin — bundler plugin
  anvil-cli/               # @anvil-di/anvil-cli — native binary launcher shim
  anvil-codegen-wasm/      # @anvil-di/anvil-codegen-wasm — WASM codegen
  bellows/                 # @anvil-di/bellows — runtime types + controller stubs
  bellows-cli/             # @anvil-di/bellows-cli — bellows binary launcher shim
  bellows-openapi/         # @anvil-di/bellows-openapi — PostBuildHook for OpenAPI
  bellows-openapi-cli/     # @anvil-di/bellows-openapi-cli — openapi binary shim
  anvil-cli-<os>-<arch>/           # @anvil-di/anvil-cli-* — per-platform native binaries
  bellows-cli-<os>-<arch>/         # @anvil-di/bellows-cli-* — per-platform native binaries
  bellows-openapi-cli-<os>-<arch>/ # per-platform native binaries
docs/
  architecture.md, ir.md, codegen.md, validation.md, cli.md, bellows-build-plan.md
  adr/            # Architecture Decision Records
tests/fixtures/   # golden-file tests for the codegen pipeline
examples/         # working sample apps, exercised in CI
```

See [`docs/architecture.md`](./docs/architecture.md) for the full pipeline and [`docs/adr/`](./docs/adr/) for the design decisions.

## Roadmap

| Milestone | Scope | Status |
| --------- | ----- | ------ |
| M0  | Workspace scaffolding + CI | done |
| M1  | Stage-3 decorator parsing into IR | done |
| M2  | Cross-file symbol resolver (tsconfig-aware) | done |
| M3  | Graph construction + validation (missing/cycle/duplicate) | done |
| M4  | First codegen: `@Component` + `@Module` + `@Provides` | done |
| M5  | CLI: `build` / `watch` / `check` / `explain` | done |
| M6  | `@Inject` ctor + `@Singleton` — **v0.1 release** | done |
| M7  | `@Binds` interface aliasing | done |
| M8  | `@Subcomponent` support | done |
| M9  | `@IntoSet` set multibindings | done |
| M10 | `ModulePath.original`: preserve bare node_modules specifiers | done |
| M11 | Subcomponent factory params; `prune_unreachable_bindings` | done |
| M12 | Async `@Provides`: `Promise<T>` return, `_resolve` phase | done |
| M13 | WASM build; `anvil-unplugin` wasm mode | done |
| Bellows M1 | `@anvil-di/bellows` runtime types + Stage-3 controller stubs | done |
| Bellows M2 | `anvil-bellows` CLI — static `routes.module.ts` emitter | done |
| Bellows M3 | Type-driven `safeParse` validation prologues | done |
| Bellows M4 | `anvil-unplugin` `PreBuildHook`/`PostBuildHook` pipeline | done |
| Bellows M5 | `anvil-bellows-openapi` CLI — OpenAPI spec generation | done |
| M14+ | Token bindings, `@Named`, optional injection, v0.1 stable | planned |

## Building from source

```bash
cargo test --workspace      # Rust unit + snapshot + integration tests
pnpm install
pnpm -r test                # TypeScript runtime tests
pnpm -r build               # Build all npm packages
```

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the full developer workflow and testing pyramid.

## License

[Apache-2.0](./LICENSE) — chosen to match Dagger and remain GPL-compatible.
