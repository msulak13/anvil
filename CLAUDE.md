# CLAUDE.md

Project memory for Claude Code sessions working in this repository. Updated at the end of each milestone.

## Project shape

`tsdi` is a code-generation TypeScript dependency injection framework. The codegen toolchain is in Rust; the user-facing decorator runtime is a tiny TypeScript package.

Two parallel workspaces share the root:
- **Cargo workspace** — `Cargo.toml` declares `members = ["crates/*"]`.
- **pnpm workspace** — `pnpm-workspace.yaml` declares `packages/*` and `examples/*`.

Both must compile cleanly for CI to pass.

## Crate responsibilities (load-bearing)

| Crate           | Role                                                                                   | Notes                                                       |
| --------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `tsdi-core`     | IR (`Key`, `Binding`, `Provider`, `Scope`, `ModuleDecl`, `ComponentDecl`), graph, validation | No TS knowledge. No I/O. Pure data + rules.                 |
| `tsdi-parser`   | Read TS files via Oxc, extract decorators, build IR                                    | Owns import-map resolution and `Key` minting.               |
| `tsdi-codegen`  | Walk validated IR, emit `*.tsdi.ts` via TS-string → `oxc_parser` → `oxc_codegen`       | Parser validates structure; codegen canonicalizes formatting. |
| `tsdi-cli`      | `tsdi` binary; orchestrates parse → validate → emit; renders diagnostics via `miette`  | Watch mode uses `notify` (M5+).                             |

If a change blurs these boundaries (e.g. teaching `tsdi-core` about Oxc), stop and reconsider — that's a sign the design has drifted.

## Invariants

1. **No runtime reflection.** The runtime `tsdi` package's decorators are no-ops at runtime; all semantics live in the Rust toolchain.
2. **Generated code is plain `.ts`** — never `.js`+`.d.ts`. The user's own `tsc` is the final validator.
3. **Type identity is `(absolute module path, exported name)`**, derived via the file's import map. We do **not** invoke a TypeScript type checker. Non-class deps go through `Token<T>` (deferred to v0.2 / M7).
4. **One generated file per `@Component`**, co-located with the source as `<name>.tsdi.ts`.
5. **TC39 Stage-3 decorators only.** No `experimentalDecorators`. See `docs/adr/0002-stage3-decorators-only.md`.
6. **Apache-2.0 license.** All new code carries this license; no GPL/AGPL deps without explicit user approval.
7. **Oxc, not SWC.** See `docs/adr/0001-oxc-vs-swc.md`. Falling back to SWC requires a new ADR.

## Don't do

- **Don't add the Oxc dependency to `tsdi-core`.** Core is parser-agnostic by design.
- **Don't introduce `reflect-metadata`** anywhere — including in test helpers. The whole point is to avoid it.
- **Don't add `experimentalDecorators` to any `tsconfig.json`.** All sample/test/fixture code is Stage-3.
- **Don't write generated code to disk during unit tests.** Use `insta` snapshots in-memory; reserve disk-touching tests for integration suites under `tests/`.
- **Don't emit unparsed TS.** `tsdi-codegen` builds TS source as a string then runs it through `oxc_parser` + `oxc_codegen` before returning. The parse step is the structural-correctness check (it rejects malformed output before it ever lands on disk); the codegen step canonicalizes formatting. Returning a raw, unparsed string defeats both. Driving `oxc_ast::AstBuilder` directly is also acceptable but not required — the parse-and-print pipeline is the sanctioned default for emission. The banner is the one exception: it's prepended to the codegen output as a comment, since `oxc_codegen` strips comments.
- **Don't shell out to `tsc`** from Rust as a build step. Generated code is validated by the user's tsc; our Rust tests use `assert_cmd` to drive `npx tsc --noEmit` only in fixture-level integration tests.

## Tooling

- Rust: pinned to `1.95.0` via `rust-toolchain.toml` (Oxc 0.127 needs >=1.93). Components: `rustfmt`, `clippy`, `rust-src`.
- Node: `>=20`. Currently used: 24.x.
- pnpm: `10.33.2` (set via `packageManager` in `package.json`).
- Lints: `cargo clippy -- -D warnings`, `cargo fmt --check`, both gating in CI.

## Testing layers (see `CONTRIBUTING.md` for details)

1. Rust unit tests (every crate). Target: `cargo test --workspace` < 5s in M0.
2. `insta` snapshots for IR (M1+), diagnostics (M3+), and emitted TS (M4+).
3. Golden-file fixtures under `tests/fixtures/<case>/{input/, expected/}` (M3+).
4. End-to-end TS validation via `assert_cmd` invoking `npx tsc --noEmit` and `npx vitest run` against fixture output (M4+).
5. Examples-as-tests: every `examples/*` builds in CI on every push (M4+).

## Milestone log

- **M0** — Cargo + npm workspaces compile. Smoke tests in every crate. Runtime package exports Stage-3 decorator stubs and `Token<T>` with TSDoc + Vitest coverage. Top-level docs and ADRs seeded.
- **M1** — `tsdi-parser` parses TS via Oxc 0.127 and lowers `@Module`/`@Provides`/`@Inject`/`@Component`/`@Singleton` into the IR's `ParsedFile`. Single-file import map mints `Key::Class` from import specifiers; same-file references use `ModulePath::SAME_FILE = "<self>"` until M2's resolver normalizes them. Parser errors enumerated in `decorators::ExtractError`; `miette` rendering deferred to M3. 8 `insta` snapshot/error tests in `crates/tsdi-parser/tests/extract_snapshots.rs`.
- **M2** — `tsdi_parser::symbols` adds `ProjectResolver` (wrapping `oxc_resolver` 11 with `.ts/.tsx/.d.ts/.js/.jsx/.json` extensions and optional tsconfig `paths` discovery) and `ProjectGraph::build_from_entry`, which walks the transitive `.ts`/`.tsx` import graph from a `@Component` entry, parses each file via `parse_file`, and rewrites every `ModulePath` in the resulting IR to an absolute canonical path (`SAME_FILE` → file's own abs path; raw specifier → `oxc_resolver::resolve(parent_dir, spec)`). Files reachable only through `node_modules` are resolved but **not** recursed into. 4 integration tests in `crates/tsdi-parser/tests/symbols_resolution.rs` cover relative imports, tsconfig `paths`, barrel re-exports, and `node_modules` packages using on-disk `tempfile` fixtures.
- **M3** — `tsdi-core::graph::build_and_validate(GraphInput) -> (DependencyGraph, Vec<Diagnostic>)` aggregates a component's bindings (provides from referenced modules + project-wide `@Inject` self-bindings), builds a petgraph `DiGraph<Key, ()>`, and emits structured `Diagnostic`s for `MissingBinding` (entry-point and dep flavors), `Cycle` (Tarjan SCC, including self-loops), `Duplicate` (key declared by ≥ 2 sources), and `ScopeMismatch` (`Singleton` binding inside non-`Singleton` component). IR gained `SourceSpan { path, start, end }` and a `source` field on each of `Binding`/`ModuleDecl`/`EntryPoint`/`ComponentDecl`; the parser populates them via a `to_ir_span` helper, and the M2 resolver canonicalizes the `path` field alongside `ModulePath`. `tsdi-cli` ships a real `tsdi check --entry <ts.ts> [--tsconfig <path>]` that pipes through M2 + the new validator and renders diagnostics through `miette::MietteDiagnostic` (one `NamedSource` per diagnostic; cross-file labels become `help` notes). Exit codes: `0` ok, `1` validation failed, `2` tooling error. Tests: 9 unit tests in `crates/tsdi-core/src/graph.rs` + 5 CLI integration tests in `crates/tsdi-cli/tests/check_command.rs` covering all four diagnostic kinds against tempdir fixtures.
- **M4** — `tsdi-codegen::emit_component(component, modules, inject_classes, version) -> Result<String>` emits one `<component>.tsdi.ts` per `@Component`. Pipeline: `build_and_validate` → topological order (DFS post-order, lex tie-break on `Name@module`) → string-built TS → `oxc_parser::Parser` (structural-correctness gate) → `oxc_codegen::Codegen` (canonical formatting) → banner + `// Source:` prepend. Imports are reconstructed by computing relative paths from the output dir to each referenced absolute module path (M2 has already canonicalized everything). M4 supports `Scope::Unscoped` only — `Scope::Singleton` returns `EmitError::SingletonNotYetSupported` until M6 adds the `??=` lazy-cache field. `tsdi-cli` ships `tsdi build --entry <ts.ts> [--tsconfig <path>]` with the same exit-code contract as `check`; on validation failure no files are written. Tests: 4 snapshot/error tests in `crates/tsdi-codegen/tests/emit_snapshots.rs`, 2 CLI tests in `crates/tsdi-cli/tests/build_command.rs`, and 1 golden-file fixture test in `crates/tsdi-cli/tests/fixtures.rs` covering `tests/fixtures/01_simple_provides/{input,expected}` (set `BLESS=1` to refresh the expected file).

### M4 gotchas
- `oxc_codegen::Codegen::build` strips comments from the printed AST. The banner and `// Source:` lines must be **prepended** to the codegen output as text — they cannot be embedded into the parsed program.
- `@Inject` is a **class-level** decorator, not a constructor decorator. TC39 Stage-3 decorators don't apply to constructors. The parser rejects the legacy `@Inject constructor(...)` placement with `ExtractError::InjectOnConstructor`. When `@Inject` sits on the class, the parser still walks the `constructor` method's parameter types to populate `Provider::InjectCtor.deps`. The emitted `.tsdi.ts` is decorator-free regardless.
- Computing relative paths via `std::path::Path::components` requires both inputs to be absolute, which they are post-M2 — but on Windows the components include a `Prefix(C:)` and `RootDir`, so the simple "common-prefix length" comparison still works. Tests use `tempfile::TempDir` paths so they're truly absolute on both OSes.
- `assert_cmd::Command::cargo_bin("tsdi")` rebuilds the binary on every test invocation — keep the binary's compile time low (no heavy `dev-dependencies` linked into the bin) or test runs balloon.
- Golden-file fixture tests use a `BLESS=1` env-var convention rather than `insta`'s `--review` workflow because the fixture lives outside the snapshot-managed directory tree (it's a deliberate on-disk artifact, not an internal test detail).

### M3 gotchas
- `petgraph::graphmap::DiGraphMap` requires `Ord` keys; `Key` is only `Hash + Eq`. Use `DiGraph<Key, ()>` + a `HashMap<Key, NodeIndex>` lookup instead.
- A SCC of size 1 is *not* a cycle unless `graph.contains_edge(ix, ix)`. Without that check Tarjan reports every node as cycling.
- `miette::MietteDiagnostic::with_labels` attaches `LabeledSpan`s relative to a single `NamedSource` (set via `Report::with_source_code`). For multi-file diagnostics, embed cross-file labels in the `help` string instead of adding `LabeledSpan`s pointing into the wrong source.
- Adding `source: SourceSpan` to IR types invalidates every M1 IR snapshot. Re-accept with `INSTA_UPDATE=always cargo test -p tsdi-parser`.
- `predicates::str::contains(...).and(...)` requires `use predicates::prelude::PredicateBooleanExt;`.
- `clippy::too_many_lines` defaults to 100; the validator entry point exceeded that. Split into `aggregate_bindings` / `build_petgraph` / `detect_cycles` / `detect_scope_mismatches` instead of an `#[allow]`.

### M1 gotchas
- Oxc 0.127's `FormalParameter` has `type_annotation` directly on the parameter — **not** on `pattern` (`pattern` is a `BindingPattern` enum with no fields).
- Oxc abstract methods are `MethodDefinition` with `r#type == MethodDefinitionType::TSAbstractMethodDefinition` (and `kind == Method`). Distinguishing abstract vs concrete via `kind` alone is wrong.
- `oxc_ast::ast::TSTypeName::IdentifierReference` is what a bare `T` in a type annotation looks like; member-expression types (`ns.T`) are not in scope for v0.1.
- Side-effect-only imports (`import "./register"`) have `decl.specifiers == None` — handle that explicitly when building the import map.

### M2 gotchas
- `oxc_resolver::Resolver::resolve` takes the **directory** of the importing file (not the file itself). Passing the file path silently returns confusing results.
- The default `extensions` list does *not* include `.ts`/`.tsx`. If you forget to set them, every `import "./foo"` fails. Order matters too: list `.ts` before `.js` so source wins over emitted output.
- `Resolution::full_path()` returns a `PathBuf`. `path()` returns a `&Path`. Both are absolute but use the OS-native canonical form on Windows (UNC `\\?\` prefixes from `fs::canonicalize` are normalized away by `oxc_resolver`).
- Barrel re-exports (`export { X } from "./y"`) resolve to the barrel file, **not** to the underlying declaration. M3's binding-graph walker is the layer that follows re-exports — the M2 resolver only normalizes specifier→absolute-path, it does not chase declarations.
- `tsconfig.json` `paths` only takes effect when `ResolveOptions::tsconfig` is `Some(TsconfigDiscovery::Manual { config_file, .. })` — there's no auto-discovery from arbitrary cwds.
