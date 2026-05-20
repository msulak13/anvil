# CLAUDE.md

Project memory for Claude Code sessions. Updated at the end of each milestone.

## Project shape

Two parallel workspaces share the root:
- **Cargo workspace** — `Cargo.toml` declares `members = ["crates/*"]`
- **pnpm workspace** — `pnpm-workspace.yaml` declares `packages/*` and `examples/*`

The Rust toolchain (`tsdi-*` crates) does all codegen. npm packages (`anvil-*`) are runtime stubs, bundler plugins, and binary distributions. Both workspaces must compile cleanly for CI to pass.

## Crate responsibilities

| Crate | Role | Notes |
|---|---|---|
| `tsdi-core` | IR (`Key`, `Binding`, `Provider`, `Scope`, `ModuleDecl`, `ComponentDecl`), graph, validation | No TS knowledge. No I/O. Pure data + rules. |
| `tsdi-parser` | Read TS files via Oxc, extract decorators, build IR | Owns import-map resolution and `Key` minting. |
| `tsdi-codegen` | Walk validated IR, emit `*.anvil.ts` via TS-string → `oxc_parser` → `oxc_codegen` | Parser validates; codegen canonicalizes formatting. |
| `tsdi-cli` | `anvil` binary; orchestrates parse → validate → emit; renders diagnostics via `miette` | Watch mode uses `notify` (M5+). |
| `anvil-bellows` | Parse `@Controller` files (Oxc), emit `routes.module.ts`; `anvil-bellows` binary | Static mode only; M3 adds `--tsc`. |

If a change blurs these boundaries (e.g. teaching `tsdi-core` about Oxc), stop and reconsider.

## Invariants

1. **No runtime reflection.** Decorators are no-ops at runtime; all semantics live in the Rust toolchain.
2. **Generated code is plain `.ts`** — never `.js`+`.d.ts`. The user's own `tsc` is the final validator.
3. **Type identity is `(absolute module path, exported name)`** derived via the import map. We do not invoke a TypeScript type checker.
4. **One generated file per `@Component`**, co-located as `<name>.anvil.ts`.
5. **TC39 Stage-3 decorators only.** No `experimentalDecorators`. See `docs/adr/0002-stage3-decorators-only.md`.
6. **Apache-2.0 license.** No GPL/AGPL deps without explicit approval.
7. **Oxc, not SWC.** See `docs/adr/0001-oxc-vs-swc.md`. Switching requires a new ADR.

## Don't do

- **Don't add Oxc to `tsdi-core`.** Core is parser-agnostic by design.
- **Don't introduce `reflect-metadata`** anywhere — including in test helpers.
- **Don't add `experimentalDecorators` to any `tsconfig.json`.** All code is Stage-3.
- **Don't write generated code to disk during unit tests.** Use `insta` snapshots; reserve disk-touching tests for integration suites under `tests/`.
- **Don't emit unparsed TS.** Build as a string, parse through `oxc_parser` + `oxc_codegen`. The banner is the exception — prepend it as text after codegen (Codegen strips comments).
- **Don't shell out to `tsc` from Rust.** Use `assert_cmd` to invoke `npx tsc --noEmit` only in fixture-level integration tests.

## Tooling

- Rust: pinned `1.95.0` via `rust-toolchain.toml` (Oxc 0.127 needs ≥1.93). Components: `rustfmt`, `clippy`, `rust-src`.
- Node: `>=20` (currently 24.x). pnpm: `10.33.2`.
- Lints: `cargo clippy -- -D warnings`, `cargo fmt --check`, both gating in CI.

## Testing layers

1. Rust unit tests — target `cargo test --workspace` < 5 s.
2. `insta` snapshots for IR, diagnostics, and emitted TS.
3. Golden-file fixtures under `tests/fixtures/<case>/{input/, expected/}` — refresh with `BLESS=1`.
4. End-to-end: `assert_cmd` invoking `npx tsc --noEmit` + `npx vitest run` against fixture output.
5. Examples-as-tests: every `examples/*` builds in CI.

## Packages

| Directory | npm name | Role |
|---|---|---|
| `packages/anvil` | `@anvil-di/anvil` | Runtime decorator stubs (`@Inject`, `@Provides`, `@Module`, `@Component`, `@Singleton`, `@Binds`, `@Subcomponent`, `@IntoSet`). All no-ops. `@Singleton` has class + method overloads so `@Singleton @Provides` typechecks. |
| `packages/anvil-unplugin` | `@anvil-di/anvil-unplugin` | Bundler plugin (vite/rollup/webpack/rspack/esbuild). Runs `anvil build` on `buildStart`; watch mode debounces file changes. `mode: "wasm"` runs codegen in-process via the WASM package. 5 tests, all green. |
| `packages/anvil-cli` | `@anvil-di/anvil-cli` | Launcher shim. Resolves the native binary via `optionalDependencies` or the `TSDI_CLI_BIN` env var. |
| `packages/anvil-cli-<platform>-<arch>` | `@anvil-di/anvil-cli-*` | Native binaries: darwin-arm64, darwin-x64, linux-arm64, linux-x64, win32-x64. Only win32-x64 committed; others filled by `release-cli.yml`. |
| `packages/anvil-codegen-wasm` | `@anvil-di/anvil-codegen-wasm` | WASM build of `crates/tsdi-codegen-wasm`. `wasm-opt = false` — oxc emits `memory.copy` that bundled wasm-opt rejects. 1.4 MB unoptimized is fine. |
| `packages/bellows` | `@anvil-di/bellows` | Runtime types + decorator stubs for NestJS-style controller codegen (`Validator<T>`, `Body<S>`, `Query<S>`, `Params<S>`, `Responds<S>`, `withJsonSchema`, `RouteDefinition`, + 11 Stage-3 decorator stubs). Also exports `PreBuildHook` + `bellowsCodegen()`. Bellows M1+M2. |
| `packages/bellows-cli` | `@anvil-di/bellows-cli` | Launcher shim for the `anvil-bellows` native binary. Mirrors `anvil-cli`. Env var: `ANVIL_BELLOWS_CLI_BIN`. |
| `packages/bellows-cli-<platform>-<arch>` | `@anvil-di/bellows-cli-*` | Native binaries: darwin-arm64, darwin-x64, linux-arm64, linux-x64, win32-x64. Filled by CI release workflow. |
| `packages/bellows-openapi` | `@anvil-di/bellows-openapi` | Exports `bellowsOpenApi()` PostBuildHook factory. Runs `anvil-bellows-openapi` via env var or platform package. Bellows M5. |
| `packages/bellows-openapi-cli` | `@anvil-di/bellows-openapi-cli` | Launcher shim for the `anvil-bellows-openapi` native binary. Mirrors `bellows-cli`. Env var: `ANVIL_BELLOWS_OPENAPI_CLI_BIN`. |
| `packages/bellows-openapi-cli-<platform>-<arch>` | `@anvil-di/bellows-openapi-cli-*` | Native binaries: darwin-arm64, darwin-x64, linux-arm64, linux-x64, win32-x64. Only win32-x64 committed; others filled by CI release workflow. |

## Milestone log

| Milestone | Summary |
|---|---|
| M0 | Workspaces compile; smoke tests; runtime stubs; docs/ADRs |
| M1 | Parser: `@Module`/`@Provides`/`@Inject`/`@Component`/`@Singleton` → `ParsedFile` IR |
| M2 | Symbol resolution: `ProjectResolver`, `ProjectGraph::build_from_entry`, `ModulePath` canonicalization |
| M3 | Graph validation: `build_and_validate`, diagnostics (MissingBinding, Cycle, Duplicate, ScopeMismatch), `anvil check` CLI |
| M4 | Codegen: `emit_component` → `*.anvil.ts`, `anvil build` CLI, golden-file fixture runner |
| M5 | CLI surface: build/check/watch/explain, config file, auto-discovery, debounced watch |
| M6 | Singleton caching: `??=` cache fields in emitter; v0.1 release |
| M7 | `@Binds` interface aliasing |
| M8 | `@Subcomponent` support (zero-arg factory methods) |
| M9 | `@IntoSet` multibindings: `Key::Set`, `SetMultibinding` synthesized in aggregator |
| M10 | `ModulePath.original`: preserve bare node_modules specifiers through codegen round-trip |
| M11 | Subcomponent factory params; `prune_unreachable_bindings`; DFS topo guard fix |
| M12 | Async `@Provides`: `Promise<T>` return, `_resolve` phase, async `create()` |
| M13 | WASM build: `tsdi-codegen-wasm` crate + npm package, `MapResolver`, `anvil-unplugin` wasm mode |
| Bellows M1 | `packages/bellows`: runtime types, `Validator<T>` interface, wrapper types, `withJsonSchema`, `RouteDefinition`, Stage-3 decorator stubs (Controller, Get, Post, Put, Delete, Patch, Middleware, Tag, Returns, Security, Deprecated) |
| Bellows M2 | `crates/anvil-bellows`: Rust crate + `anvil-bellows` binary — static Oxc parser for `@Controller`/`@Get`/etc., `routes.module.ts` emitter, `PreBuildHook` interface, `bellowsCodegen()` factory. `packages/bellows-cli` + platform stubs mirror the `anvil-cli` distribution model. |
| Bellows M3 | Type-driven adapter generation in `crates/anvil-bellows`. Parser detects `Body<typeof S>`, `Query<typeof S>`, `Params<typeof S>`, `Request`, `Response`, `Responds<typeof S>`, `Promise<Responds<typeof S>>` from the Oxc AST. Codegen emits `safeParse` validation prologues; routes with any `Unknown` param fall back to v0.1 passthrough. Fixture `03_schema_params` with tsc validation. |
| Bellows M4 | `packages/anvil-unplugin`: `PreBuildHook`/`PostBuildHook` interfaces exported; `AnvilPluginOptions` gains `preBuild`/`postBuild` arrays. `buildStart` runs: preBuild hooks → anvil build → postBuild hooks. Watch mode accumulates changed files across the debounce window and re-runs hooks via `shouldRerun`. 7 new tests (order, watch trigger, watch skip, postBuild cascade). |
| Bellows M5 | `crates/anvil-bellows-openapi` Rust crate + CLI (`--entry`, `--output`, `--format json\|yaml`, `--config`). Parser extensions: `@Tag`, `@Security`, `@Deprecated`, `@Returns(N)` on controllers/routes. `packages/bellows-openapi` npm package exports `bellowsOpenApi()` PostBuildHook. `packages/bellows-openapi-cli` + win32-x64 platform stub mirror the `bellows-cli` distribution model. |

## Load-bearing implementation notes

Gotchas that cause silent misbehavior or subtle bugs if forgotten.

### Parser (`tsdi-parser`)

- `FormalParameter` has `type_annotation` directly — **not** on `pattern` (Oxc 0.127).
- Abstract methods: `r#type == MethodDefinitionType::TSAbstractMethodDefinition` (and `kind == Method`). `kind` alone is insufficient.
- `TSTypeName::IdentifierReference` is what a bare `T` looks like; member-expression types (`ns.T`) not in scope for v0.1.
- Side-effect-only imports (`import "./register"`) have `decl.specifiers == None` — handle explicitly.
- `@Inject` is **class-level**, not constructor-level. The legacy `@Inject constructor(...)` form is rejected with `InjectOnConstructor`.
- TC39 Stage-3 **cannot** decorate abstract methods (TS1249). `@Binds` requires a `static` method with a trivial `return impl;` body — codegen ignores the body.
- `async constructor` is rejected by Oxc before the parser sees it; `AsyncInjectCtor` is defense-in-depth with no test.
- `Set<T>` annotations need a separate `ts_type_to_key` helper (bare `TSType`) distinct from `type_annotation_to_key` (`TSTypeAnnotation`).

### Symbol resolution (`tsdi-parser::symbols`)

- `oxc_resolver::Resolver::resolve` takes the **directory** of the importing file — passing the file path silently returns wrong results.
- Default extensions do **not** include `.ts`/`.tsx` — must set them. Order: `.ts` before `.js` so source wins over compiled output.
- `ModulePath` is `{ abs: String, original: Option<String> }` with custom `PartialEq`/`Hash` on `abs` only. **Do not derive** — it would break duplicate-binding detection, cycle detection, and the subcomponent parent-bindings fallback.
- Constructor matrix: `same_file()` (no original), `from_specifier(s)` (parser, seeds both fields), `from_abs(p)` (tests/tooling, no original). Never unconditionally `.unwrap()` `original` in codegen.
- `is_node_modules()` is a substring match on `abs` — good enough for v0.1.
- **Every key-bearing IR field must be recursed into by the M2 resolver.** A missed field keeps its raw specifier while others have absolute paths, silently breaking `Key` equality. The canonical failure mode: M11's `EntryPoint.factory_params[i].key`.
- Barrel re-exports resolve to the barrel file, not the underlying declaration. Re-export chasing is the graph layer's job.
- `tsconfig.json` `paths` only activates with `ResolveOptions::tsconfig = Some(TsconfigDiscovery::Manual { ... })`.

### Graph (`tsdi-core::graph`)

- Use `DiGraph<Key, ()>` + `HashMap<Key, NodeIndex>` — **not** `DiGraphMap` (requires `Ord`; `Key` is only `Hash + Eq`).
- A SCC of size 1 is **not** a cycle unless `graph.contains_edge(ix, ix)`.
- `Provider::SetMultibinding` is synthesized in `aggregate_bindings` — the parser never produces it. The element-type lift (raw `Plugin` → `Key::Set { Plugin }`) happens in the aggregator. Lifting in the parser triggers the duplicate-binding rule.
- `Provider::FactoryParam` is graph-synthesized — the parser never produces it. The `rewrite_binding` arm in `symbols.rs` is unreachable but kept for completeness.
- `Binding.deps` for `Provider::Binds { target }` **must** include `target`. Without it, the alias factory can emit before the impl factory and reference undefined `getX()`.
- `prune_unreachable_bindings` is load-bearing. Without it, child-only `@Inject` classes leak to the parent dagger as public factories, bypassing the subcomponent pattern.
- `build_child_graph` order: inject factory-param bindings **before** the prune step, or the prune drops them as unreachable transitive deps.
- **DFS topo guard:** only push a key to the topo array when it has a local binding (`if let Some(b) = graph.bindings.get(k)`). Inherited keys in the topo cause panics when code indexes into `graph.bindings`. Load-bearing in both M11 and M12's `_resolve` body.
- Multibinding contributions map is keyed on the synthesized `Key::Set { element }`, not the raw element key.
- Parent factories exposed to a child class **must not be `private`** (TS2341). Track `parent_keys_exposed = ⋃ child.inherited_keys` and omit the `private` modifier for those.
- Async `@Provides` is rejected on non-`@Singleton` components but **not** on Unscoped subcomponents (per-call awaits match per-call subcomponent factory pattern).

### Codegen (`tsdi-codegen`)

- `oxc_codegen::Codegen::build` **strips all comments**. The file banner must be prepended as text after codegen runs.
- Singleton cache fields must be `T | undefined` — `strictPropertyInitialization` rejects uninitialized bare-typed fields.
- Cache field names: `_` + lowerCamel (`HotPump` → `_hotPump`, not `_hotpump`).
- `??=` is the correct primitive for singleton caching — don't expand to a 3-line if/assign/return.
- `static async _resolve(d: DaggerX)` — **not** `d: this` — TS2526 forbids `this` types on static methods. Pass `self_dagger: &str` (the literal class name) into codegen.
- The `_resolve` body rewrites `this.` → `d.` in each binding's body expression. This is a closed transform that works because all dep-calls use `this.<getX>()`. If a future provider uses `this.` for anything else, re-examine.
- `binding_is_async(&Key)` detects async per-binding; don't treat every binding in an async graph as async.

### CLI (`tsdi-cli`)

- `notify` watcher is non-debounced. The 100 ms `recv_timeout` is required — without it, one save triggers 3–10 rebuilds.
- Canonicalize watch events before comparing to source-closure paths (both sides go through the same `canonicalize`, so UNC prefixes on Windows stay consistent).
- `TSDI_WATCH_ITERATIONS=N` bounds the watch loop for test determinism; tests poll `child.try_wait()` with a deadline.
- `--entry` and `--config` are mutually exclusive at the clap level (`conflicts_with`).

### Extending IR types

Adding a new `Key` or `Provider` variant, or a new `Binding` field, is a codebase-wide break. Use `cargo build` to find every site — there is no central registry.

- New `Key` variant: `graph.rs`, `symbols.rs::rewrite_key`, codegen helpers (`type_string_of`, `factory_name_for`, `cache_field_for`), `cli::explain`.
- New `Provider` variant: `graph.rs`, `tsdi-codegen::emit_component`, `symbols.rs::rewrite_binding`, `cli::explain::provider_label`.
- New `Binding` field: every `Binding { ... }` literal (parser, codegen tests, graph test helpers).
- Runtime stubs in test fixtures must export every decorator. Missing stubs surface as `oxc_resolver` errors, not TS type errors.

### Bellows examples (`examples/todo-app`)

- `Returns(_status, _schema?)` — `_schema` is optional; `@Returns(204)` with one arg is valid Stage-3 TypeScript.
- `@anvil-di/bellows` devDeps use `@types/express@^5`; consumers must also use express 5 types or the `RouteDefinition.handler` type will mismatch across pnpm's per-package node_modules.
- Zod `.optional()` emits `string | undefined` on the inferred type; service methods must accept `T | undefined` explicitly when `exactOptionalPropertyTypes: true`.
- The generated `routes.module.ts` is checked into the repo (same pattern as `.anvil.ts` files). `routes.module.ts` lists static methods by name; `server.ts` calls them directly since the decorators are no-ops at runtime.
