# `@anvil-di/anvil-cli`

Native binary launcher for [`@anvil-di/anvil`](https://github.com/msulak13/tsdi) — installs the right prebuilt CLI for your platform, exposes it as an `anvil` command, and provides a programmatic API for tooling that needs to invoke codegen (like [`@anvil-di/anvil-unplugin`](https://www.npmjs.com/package/@anvil-di/anvil-unplugin)).

## Install

```bash
npm  install -D @anvil-di/anvil-cli       # adds the launcher + npm picks one platform package
pnpm add  -D @anvil-di/anvil-cli           # pnpm honors optionalDependencies by default
yarn add  -D @anvil-di/anvil-cli           # yarn honors optionalDependencies by default
```

The `@anvil-di/anvil-cli` package itself ships zero binaries. Its `optionalDependencies` field lists every supported platform; npm's `os`/`cpu` filtering installs **exactly one** of them (the one matching your machine). The CLI binary lives inside that platform package.

## Use as a command

```bash
npx anvil build --entry src/app-component.ts
npx anvil check --entry src/app-component.ts
npx anvil watch --entry src/app-component.ts
npx anvil explain --entry src/app-component.ts
```

The `anvil` shim in `node_modules/.bin` resolves the per-platform package, then `spawnSync`s the real binary with your argv pass-through.

## Use programmatically

```ts
import { resolveBinaryPath, unresolvableBinaryError } from "@anvil-di/anvil-cli";

const binary = resolveBinaryPath();
if (binary === null) {
  throw new Error(unresolvableBinaryError());
}
// `binary` is an absolute path you can pass to execFile / spawn.
```

This is what `@anvil-di/anvil-unplugin` uses by default. Any tooling that needs the same "find my platform's binary" lookup can use the same API.

## Resolution order

`resolveBinaryPath()` checks, in order:

1. **`ANVIL_CLI_BIN` env var** — if it points at an existing file, that file wins. Useful for tests, monorepo dev (point at `target/release/anvil`), and CI that builds from source.
2. **`require.resolve("@anvil-di/anvil-cli-<platform>-<arch>")`** — the per-platform npm package corresponding to the current Node process.

If neither resolves, the function returns `null` and `unresolvableBinaryError()` produces a human-readable message diagnosing why (unsupported platform, optional deps skipped during install, etc.).

## Supported platforms

| Platform | Arch | Package |
|---|---|---|
| Linux | x64 | `@anvil-di/anvil-cli-linux-x64` |
| Linux | arm64 | `@anvil-di/anvil-cli-linux-arm64` |
| macOS | x64 (Intel) | `@anvil-di/anvil-cli-darwin-x64` |
| macOS | arm64 (Apple Silicon) | `@anvil-di/anvil-cli-darwin-arm64` |
| Windows | x64 | `@anvil-di/anvil-cli-win32-x64` |

Need another? Open an issue, or build from source:

```bash
git clone https://github.com/msulak13/tsdi
cd tsdi
cargo build --release -p anvil-cli
export ANVIL_CLI_BIN=$PWD/target/release/anvil
```

## How releases work

The repo's [`release-cli.yml` GitHub Actions workflow](https://github.com/msulak13/tsdi/blob/main/.github/workflows/release-cli.yml) builds the binary on each native runner (linux-x64, linux-arm64 via cross, macos-13, macos-14, windows-latest) and publishes the matching `@anvil-di/anvil-cli-<platform>-<arch>` package along with the launcher. The launcher's `optionalDependencies` are pinned to the same version, so a single `npm install @anvil-di/anvil-cli@<version>` always pulls a coherent set.

## Why this layout

esbuild popularized this pattern because it sidesteps every alternative's failure modes:

- **No `postinstall` build step** — no Rust toolchain on user machines, no opaque "compiling…" delay during `npm install`.
- **No giant universal binary in the launcher** — `npm install @anvil-di/anvil-cli` downloads ~4MB, not ~25MB times five.
- **No conditional logic in the launcher's `package.json`** — npm does the platform filtering for us via `os`/`cpu`.
- **Hash-pinned reproducibility** — every install gets the exact binary CI built; nothing is recompiled on the user's machine.
