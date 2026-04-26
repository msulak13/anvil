# CLI reference

The `tsdi` binary, exposed by [`crates/tsdi-cli`](../crates/tsdi-cli). M0 only ships `--version` and `--help`; the full surface lands in M5.

Update this page when adding, removing, or renaming any flag — these are part of the public API.

## Subcommands

### `tsdi build` (M4+)

One-shot codegen. Parses the entry component's import closure, validates the graph, and writes one `<component>.tsdi.ts` next to each `@Component`'s source file. M4 supports `Scope::Unscoped` only; `Scope::Singleton` lands in M6.

```bash
tsdi build --entry <ts.ts> [--tsconfig <tsconfig.json>]
```

| Flag                | Default | Description                                                                              |
| ------------------- | ------- | ---------------------------------------------------------------------------------------- |
| `--entry <path>`    | —       | Required. A `.ts` file containing the `@Component` class to emit.                        |
| `--tsconfig <path>` | none    | Optional `tsconfig.json` whose `paths` / `baseUrl` are honored by the resolver.          |

Exit codes:
- `0` — success
- `1` — validation error (diagnostics on stderr); no files are written
- `2` — config or I/O error

The glob-based config (`tsdi.config.json`) and `--config` / `--project` flags ship in M5.

### `tsdi watch` (M5)

Starts a long-running watcher via `notify`. On change, only components whose dependency closure intersects the changed file are regenerated.

```bash
tsdi watch [--config ...] [--project ...]
```

Press `Ctrl+C` to exit. Acceptance target: re-emit the right `.tsdi.ts` within ~200ms on the basic example.

### `tsdi check` (M3+)

Validation only — no files are written. Useful as a pre-commit hook or a fast CI gate.

```bash
tsdi check --entry <ts.ts> [--tsconfig <tsconfig.json>]
```

Same flags and exit codes as `tsdi build`.

### `tsdi explain <Key>` (M5+)

Diagnostic helper. Prints how a key would be resolved by the current config: which file declares it, which provider satisfies it, what its transitive deps are.

```bash
tsdi explain "src/coffee/pump.ts#Pump"
```

The key syntax is `<file path>#<exported name>` for `Key::Class` and `<file path>#<token name>` for `Key::Token` (M7+).

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
