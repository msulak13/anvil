# Contributing to tsdi

Thanks for your interest. This document covers the developer workflow and the testing pyramid every change is expected to clear.

## Development environment

| Tool   | Version        | Source                                                    |
| ------ | -------------- | --------------------------------------------------------- |
| Rust   | 1.85.1         | pinned by [`rust-toolchain.toml`](./rust-toolchain.toml)  |
| Node   | ≥ 20 (24.x ok) | install via `nvm` / `fnm` / `volta`                       |
| pnpm   | 10.33.2        | pinned by `packageManager` in [`package.json`](./package.json) — Corepack will install it |

After cloning:

```bash
# Rust toolchain components install on first cargo invocation.
cargo build --workspace

# pnpm via Corepack (`corepack enable` once if needed).
pnpm install
```

## Project layout

See [`README.md`](./README.md) for the high-level layout and [`docs/architecture.md`](./docs/architecture.md) for the data flow between crates. Crate-level responsibilities are summarized in [`CLAUDE.md`](./CLAUDE.md) — keep that file accurate; deviating from it is usually a sign that a refactor went sideways.

## Testing pyramid

All five layers must pass for a PR to merge.

1. **Rust unit tests** — colocated with each module. Run with `cargo test --workspace`. Should stay under ~5 seconds in M0–M3.

2. **IR snapshot tests** (M1+) — `insta` snapshots of parsed `ModuleDecl` / `ComponentDecl` trees. Live in `crates/tsdi-parser/src/snapshots/`. Review with `cargo insta review`.

3. **Diagnostic snapshot tests** (M3+) — `insta` snapshots of rendered `miette` errors for every validation rule. Live in `crates/tsdi-core/src/snapshots/`.

4. **Codegen snapshot tests** (M4+) — `insta` snapshots of emitted TS in `crates/tsdi-codegen/src/snapshots/`. Organized by feature (modules, scopes, etc.).

5. **Golden-file fixture tests** (M3+) — `tests/fixtures/<case>/{input/, expected/}`. A single Rust integration test walks the directory, runs the pipeline on `input/`, and diffs against `expected/`. For end-to-end correctness, the same test then invokes `npx tsc --noEmit` and `npx vitest run` against the generated output via `assert_cmd`.

6. **Examples-as-tests** (M4+) — every `examples/*` is built by CI on every push. A broken example blocks release.

7. **Benchmarks** (M5+) — `criterion` benches over a 100-component synthetic project. Regressions gate CI.

## Linting and formatting

Run before committing:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
pnpm -r typecheck   # tsc --noEmit on every TS package
```

CI runs the same set on Linux, macOS, and Windows.

## Documentation expectations

Documentation is a first-class deliverable. Every milestone:

1. Update [`CLAUDE.md`](./CLAUDE.md) with any invariants or gotchas surfaced.
2. Update the relevant page in [`docs/`](./docs/) (e.g. M3 updates `validation.md`; M4 updates `codegen.md`).
3. Write an ADR in [`docs/adr/`](./docs/adr/) if the milestone made a non-obvious architectural choice.
4. Add rustdoc / TSDoc to every new public item. CI runs `cargo doc --no-deps -D warnings` and `cargo test --doc`.
5. Refresh `examples/*/README.md` when codegen output changes.

## Commit / PR style

- One change per PR. Keep refactors and feature work separate.
- Reference the milestone in the title when applicable: `[M3] add cycle-detection rule`.
- Include the rationale in the PR body, not the commit message — commit messages should describe **what** changed; PR descriptions describe **why**.

## Filing issues

For design questions or proposals, prefer opening an ADR draft as a PR over a long-form issue. ADRs preserve the decision trail; issues tend to get archived.
