# Architecture

`anvil` is a Dagger-style dependency injection framework for TypeScript. Its toolchain is a Rust CLI that reads decorated TS source, builds an internal binding graph, validates it, and emits plain `.ts` files containing the wired graph. The user's own `tsc` then compiles everything as usual.

This document is the canonical map of the system. Update it whenever a milestone adds a new pipeline stage.

## Pipeline

```
┌──────────────┐   ┌─────────────┐   ┌──────────────┐   ┌────────────┐   ┌─────────────────┐
│  user .ts    │──▶│ anvil-parser │──▶│  anvil-core   │──▶│ anvil-core  │──▶│  anvil-codegen   │
│  (decorated) │   │   (Oxc)     │   │     IR       │   │ validate   │   │ emit *.anvil.ts  │
└──────────────┘   └─────────────┘   └──────────────┘   └────────────┘   └─────────────────┘
                                                                                  │
                                                                                  ▼
                                                                        ┌─────────────────┐
                                                                        │   user's tsc    │
                                                                        │   compiles all  │
                                                                        └─────────────────┘
```

All five stages are orchestrated by `anvil-cli`, which is the only crate that performs I/O and renders diagnostics.

## Crate responsibilities

### `anvil-core` — IR, graph, validation

- Has **no knowledge of TypeScript syntax** and performs **no I/O**.
- Pure data + rules. Imports nothing from `oxc_*`.
- Public modules:
  - `ir` — `Key`, `Binding`, `Provider`, `Scope`, `ClassRef`, `ModuleDecl`, `ComponentDecl`, `EntryPoint`, `SourceSpan`.
  - `graph` — `petgraph`-backed dependency graph + `build_and_validate(GraphInput) -> (DependencyGraph, Vec<Diagnostic>)` (M3).
  - `validate` — `Diagnostic` / `DiagnosticKind` data types covering missing, cycle, duplicate, and scope-mismatch (M3). Rendering happens in `anvil-cli`.

### `anvil-parser` — TS → IR

- Reads `.ts` files via `oxc_parser` and visits the AST.
- Recognizes Stage-3 decorators: `@Module`, `@Provides`, `@Inject`, `@Component`, `@Singleton`.
- Resolves identifiers through the file's import map (M1) and across files via `oxc_resolver` (M2, in `src/symbols.rs`) to mint stable `Key::Class { module, name }` values whose `module` is an absolute, canonical filesystem path.
- The M2 `ProjectGraph::build_from_entry` walks the transitive `.ts`/`.tsx` import graph from a `@Component` entry, parses each file, normalizes its IR's `ModulePath`s, and aggregates the result. Files reached via `node_modules` are resolved (so their `Key`s are stable) but not parsed — runtime libraries don't contribute bindings in v0.1.
- Outputs `Vec<ModuleDecl>` and `Vec<ComponentDecl>` for the CLI to feed to validation.

Decorator-AST decisions live in [`adr/0002-stage3-decorators-only.md`](./adr/0002-stage3-decorators-only.md). Parser choice is recorded in [`adr/0001-oxc-vs-swc.md`](./adr/0001-oxc-vs-swc.md).

### `anvil-codegen` — IR → TS

- Walks a validated `ComponentDecl` plus its `ModuleDecl`s.
- Builds the TS source as a string, then pipes it through `oxc_parser` (structural check) and `oxc_codegen` (canonical formatting). M4 covers `Scope::Unscoped`; `Scope::Singleton` lands in M6.
- One generated `.anvil.ts` file per `@Component`, co-located with source. See [`adr/0004-per-component-output-file.md`](./adr/0004-per-component-output-file.md).
- Emits **`.ts` only** — never `.js`+`.d.ts`. The user's `tsc` is the final validator.

### `anvil-cli` — the `anvil` binary

- The only crate that performs disk I/O.
- Subcommands (full set lands in M5; see [`cli.md`](./cli.md)):
  - `build --entry <ts.ts> [--tsconfig <path>]` — one-shot codegen (M4)
  - `watch` — incremental regen via `notify` (M5)
  - `check --entry <ts.ts> [--tsconfig <path>]` — validation only (M3)
  - `explain <Key>` — diagnostics (M5)
- Renders `anvil-core` errors via `miette` for IDE-style output with source snippets.

## Data flow

1. **CLI loads config**, resolves `entries` glob to a set of `*-component.ts` files.
2. For each entry, **`anvil-parser` walks the import graph**, parsing every `.ts` file it touches and building two collections:
   - `ModuleDecl`s — every class annotated `@Module`.
   - `ComponentDecl`s — every class annotated `@Component`.
3. **`anvil-core` constructs a `DependencyGraph`** by walking each `ComponentDecl`'s `entry_points`, asking `anvil-core::graph` to resolve every transitive `Key` against the union of bindings contributed by the component's `modules`.
4. **`anvil-core::validate` runs all rules** (`MissingBinding`, `Cycle`, `Duplicate`, `ScopeMismatch`). On error, the CLI renders diagnostics and exits non-zero.
5. **`anvil-codegen` emits** one `*.anvil.ts` per `ComponentDecl` next to the source file.
6. **User's `tsc`** compiles both the original sources and the generated files.

## Stability boundaries

- **`anvil-core::ir`** is the **stable contract** between parser and codegen. Adding a variant requires updating both consumers, plus an entry in [`ir.md`](./ir.md).
- **Generated code shape** is documented in [`codegen.md`](./codegen.md). Changes that affect it require a version bump in the runtime package and a banner update.
- **CLI flags / config schema** are documented in [`cli.md`](./cli.md). Removing or renaming a flag is a breaking change.

## What lives where

| Concern                                       | Crate           | File(s)                                      |
| --------------------------------------------- | --------------- | -------------------------------------------- |
| `Key` minting from import map                 | `anvil-parser`   | `src/symbols.rs` (M2)                        |
| Decorator AST recognition                     | `anvil-parser`   | `src/decorators.rs` (M1)                     |
| Cycle detection                               | `anvil-core`     | `src/graph.rs::detect_cycles` (Tarjan SCC, M3) |
| Scope mismatch rule                           | `anvil-core`     | `src/graph.rs::detect_scope_mismatches` (M3) |
| Diagnostic rendering (miette)                 | `anvil-cli`      | `src/diagnostics.rs` (M3)                    |
| Component file emission (unscoped, M4)        | `anvil-codegen`  | `src/emit_component.rs`                      |
| Generated `??=` lazy singleton field          | `anvil-codegen`  | `src/emit_component.rs` (M6)                 |
| `miette` diagnostic rendering                 | `anvil-cli`      | `src/main.rs`                                |
| Config schema (`anvil.config.json`)            | `anvil-cli`      | `src/config.rs` (M5)                         |

If you find yourself adding logic that doesn't fit any of the above rows, stop and update this table — that's a sign the architecture is gaining a new dimension.
