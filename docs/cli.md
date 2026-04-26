# CLI reference

The `tsdi` binary, exposed by [`crates/tsdi-cli`](../crates/tsdi-cli). The full subcommand surface (`build`/`check`/`watch`/`explain`) is live as of M5.

Update this page when adding, removing, or renaming any flag — these are part of the public API.

## Project selection (shared by every subcommand)

Every subcommand needs to know which `@Component` file(s) to operate on. Three layers, in precedence order:

1. **Explicit `--entry <ts.ts>`** — single component, fastest path. Mutually exclusive with `--config`.
2. **Explicit `--config <path>`** — load a `tsdi.config.json` or `package.json` and expand its `entries` glob.
3. **Auto-discovery** — when neither flag is given, walk the cwd for `tsdi.config.json`, then `package.json` with a `tsdi` field. If nothing matches, the command errors.

`--tsconfig <path>` is always optional; when passed it overrides any `tsconfig` found in the config file.

## Subcommands

### `tsdi build` (M4+)

One-shot codegen. Parses each entry component's import closure, validates the graph, and writes one `<component>.tsdi.ts` next to each `@Component`'s source file. M4 supports `Scope::Unscoped` only; `Scope::Singleton` lands in M6.

```bash
tsdi build --entry <ts.ts> [--tsconfig <tsconfig.json>]
tsdi build --config tsdi.config.json
tsdi build                       # auto-discover from cwd
```

Exit codes:
- `0` — success
- `1` — validation error (diagnostics on stderr); no files are written
- `2` — config or I/O error

### `tsdi watch` (M5)

Starts a long-running watcher via `notify`. After an initial build, watches the project's root directory; on each change, only components whose dependency closure intersects a changed file are regenerated.

```bash
tsdi watch --entry <ts.ts> [--tsconfig <tsconfig.json>]
tsdi watch --config tsdi.config.json
tsdi watch                       # auto-discover from cwd
```

Filesystem events are debounced (100ms) to coalesce editor save bursts. The watch root defaults to the config's `rootDir` (or, with `--entry`, the entry file's parent directory).

Press `Ctrl+C` to exit. Acceptance target: re-emit the right `.tsdi.ts` within ~200ms on the basic example.

**Test knob:** set `TSDI_WATCH_ITERATIONS=N` to make the loop exit after `N` rebuild cycles. Used by `crates/tsdi-cli/tests/watch_command.rs` to drive the watcher in CI.

### `tsdi check` (M3+)

Validation only — no files are written. Useful as a pre-commit hook or a fast CI gate.

```bash
tsdi check --entry <ts.ts> [--tsconfig <tsconfig.json>]
tsdi check --config tsdi.config.json
tsdi check                       # auto-discover from cwd
```

Same exit codes as `tsdi build`. Prints `ok` to stdout when every entry validates.

### `tsdi explain <Key>` (M5+)

Diagnostic helper. Prints how a key would be resolved by the current project: which file declares it, which provider satisfies it, what its transitive deps are.

```bash
tsdi explain Pump --entry src/coffee/coffee-component.ts
```

The argument is the exported name of the binding (e.g. `Pump`). Output is a tree:

```
Pump@/abs/path/to/pump.ts (InjectCtor, Unscoped)
└─ Heater@/abs/path/to/heater.ts (InjectCtor, Singleton)
```

If multiple bindings share the same name (across components/modules), `explain` warns and uses the first match. If no binding matches, exit code `1`.

## Config file

`tsdi.config.json` (or a `tsdi` field in `package.json`):

```json
{
  "entries": ["src/**/*-component.ts"],
  "tsconfig": "./tsconfig.json",
  "outputSuffix": ".tsdi.ts",
  "rootDir": "src"
}
```

| Field          | Type     | Default                          | Description                                                                |
| -------------- | -------- | -------------------------------- | -------------------------------------------------------------------------- |
| `entries`      | string[] | `["src/**/*-component.ts"]`      | Globs (relative to repo root) matching files containing `@Component` classes. |
| `tsconfig`     | string   | `"./tsconfig.json"`              | Path to the TS project config used for module resolution.                  |
| `outputSuffix` | string   | `".tsdi.ts"`                     | Suffix appended to each component's filename to derive the generated path. |
| `rootDir`      | string   | (inferred from `tsconfig.json`)  | Source root; emitted import paths are normalized against this.             |

Globs are expanded with the [`glob`](https://docs.rs/glob/) crate; `**/` denotes any directory depth.

## Diagnostic rendering

All errors are routed through `miette`'s fancy renderer when stderr is a TTY, plain text otherwise (so CI logs stay diff-friendly). Each diagnostic carries:

- a stable code (e.g. `tsdi::missing_binding`)
- a span pointing to the offending source
- a `help:` line with the typical fix

See [`validation.md`](./validation.md) for the full list of diagnostic codes.
