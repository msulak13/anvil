# 0004 — One generated `.anvil.ts` file per `@Component`, co-located with source

**Status:** Accepted
**Date:** 2026-04-25

## Context

Dagger emits one Java file per generated artifact and places them in a separate `generated/` source root. Other compile-time DI tools (`@wessberg/DI`) inject directly into the user's `tsc` build via a transformer, with no separate output. We have several plausible shapes:

1. **One file per `@Component`, co-located** — `Foo.ts` → `Foo.anvil.ts` next to it.
2. **One file per `@Component`, in a `generated/` mirror tree** — `src/foo.ts` → `generated/src/foo.anvil.ts`.
3. **One monolithic `anvil.generated.ts`** containing every component's wiring.
4. **No file at all** — operate as a `tsc` transformer plugin.

Watch-mode efficiency, debuggability, and the user's `git` workflow all interact here.

## Decision

**Adopt approach (1): one file per `@Component`, co-located with the source file as `<name>.anvil.ts`** (suffix configurable via `outputSuffix`).

## Consequences

### Positive
- Watch-mode incremental regen is trivially correct: when a file changes, regenerate exactly the components whose dependency closure intersects it. Each regenerated file is independent.
- `git diff` in PRs shows the source change next to its generated wiring — reviewers can see effect alongside cause.
- No magic mirror-tree path math; emitted import specifiers can use the same relative paths the user wrote (`./pump`, not `../../src/pump`).
- Editor "go to definition" jumps from a generated method directly into the user's class, since they're sibling files.
- Failure mode is local: a problem with one component doesn't strand a monolithic generated file.

### Negative
- The user's source tree gains `.anvil.ts` files that look hand-edited at first glance. Mitigated by the banner and by ensuring the generator writes `// AUTO-GENERATED` as the very first line.
- Users must add `*.anvil.ts` to their `.gitignore` if they prefer not to commit generated code. v0.1 docs recommend committing them — review benefit outweighs noise.
- A single component spread across many modules still produces only one generated file (the component's own); large graphs concentrate emission in one place. This is fine but worth noting.

## Alternatives considered

- **(2) `generated/` mirror tree.** Cleaner conceptually, but breaks the "import specifiers match the user's specifiers" property and forces watch mode to maintain a parallel directory structure. Loses the inline-with-source debugging benefit.
- **(3) Monolithic file.** Forces every component change to invalidate the entire output. Watch-mode latency suffers; PR diffs become noisy.
- **(4) `tsc` transformer.** Locks us into the TS compiler pipeline (we'd need `ts-patch`/`ttypescript`), defeats the standalone-CLI architecture, and prevents using fast watch tools (we'd be tied to `tsc --watch`'s startup). Rejected even though `@wessberg/DI` chose this — they don't have Dagger-shaped scopes/subcomponents to manage.

## Revisit

Reopen this decision if:

1. Users consistently report that co-located generated files clutter their tree beyond the value provided. Switching to (2) is mechanical: only the path math in `anvil-cli` changes.
2. We want to share generated symbols across components (e.g. for code-splitting or shared singletons across components within the same scope graph) — that may push toward a hybrid where shared code lives in a sidecar file.
