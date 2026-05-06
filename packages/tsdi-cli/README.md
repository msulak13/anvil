# `tsdi-cli`

Native binary launcher for [`tsdi`](https://github.com/msulak13/tsdi) — installs the right prebuilt CLI for your platform, exposes it as a `tsdi` command, and provides a programmatic API for tooling that needs to invoke codegen (like [`tsdi-unplugin`](https://www.npmjs.com/package/tsdi-unplugin)).

## Install

```bash
npm  install -D tsdi-cli       # adds the launcher + npm picks one platform package
pnpm add  -D tsdi-cli           # pnpm honors optionalDependencies by default
yarn add  -D tsdi-cli           # yarn honors optionalDependencies by default
```

The `tsdi-cli` package itself ships zero binaries. Its `optionalDependencies` field lists every supported platform; npm's `os`/`cpu` filtering installs **exactly one** of them (the one matching your machine). The CLI binary lives inside that platform package.

## Use as a command

```bash
npx tsdi build --entry src/app-component.ts
npx tsdi check --entry src/app-component.ts
npx tsdi watch --entry src/app-component.ts
npx tsdi explain --entry src/app-component.ts
```

The `tsdi` shim in `node_modules/.bin` resolves the per-platform package, then `spawnSync`s the real binary with your argv pass-through.

## Use programmatically

```ts
import { resolveBinaryPath, unresolvableBinaryError } from "tsdi-cli";

const binary = resolveBinaryPath();
if (binary === null) {
  throw new Error(unresolvableBinaryError());
}
// `binary` is an absolute path you can pass to execFile / spawn.
```

This is what `tsdi-unplugin` uses by default. Any tooling that needs the same "find my platform's binary" lookup can use the same API.

## Resolution order

`resolveBinaryPath()` checks, in order:

1. **`TSDI_CLI_BIN` env var** — if it points at an existing file, that file wins. Useful for tests, monorepo dev (point at `target/release/tsdi`), and CI that builds from source.
2. **`require.resolve("tsdi-cli-<platform>-<arch>")`** — the per-platform npm package corresponding to the current Node process.

If neither resolves, the function returns `null` and `unresolvableBinaryError()` produces a human-readable message diagnosing why (unsupported platform, optional deps skipped during install, etc.).

## Supported platforms

| Platform | Arch | Package |
|---|---|---|
| Linux | x64 | `tsdi-cli-linux-x64` |
| Linux | arm64 | `tsdi-cli-linux-arm64` |
| macOS | x64 (Intel) | `tsdi-cli-darwin-x64` |
| macOS | arm64 (Apple Silicon) | `tsdi-cli-darwin-arm64` |
| Windows | x64 | `tsdi-cli-win32-x64` |

Need another? Open an issue, or build from source:

```bash
git clone https://github.com/msulak13/tsdi
cd tsdi
cargo build --release -p tsdi-cli
export TSDI_CLI_BIN=$PWD/target/release/tsdi
```

## How releases work

The repo's [`release-cli.yml` GitHub Actions workflow](https://github.com/msulak13/tsdi/blob/main/.github/workflows/release-cli.yml) builds the binary on each native runner (linux-x64, linux-arm64 via cross, macos-13, macos-14, windows-latest) and publishes the matching `tsdi-cli-<platform>-<arch>` package along with the launcher. The launcher's `optionalDependencies` are pinned to the same version, so a single `npm install tsdi-cli@<version>` always pulls a coherent set.

## Why this layout

esbuild popularized this pattern because it sidesteps every alternative's failure modes:

- **No `postinstall` build step** — no Rust toolchain on user machines, no opaque "compiling…" delay during `npm install`.
- **No giant universal binary in the launcher** — `npm install tsdi-cli` downloads ~4MB, not ~25MB times five.
- **No conditional logic in the launcher's `package.json`** — npm does the platform filtering for us via `os`/`cpu`.
- **Hash-pinned reproducibility** — every install gets the exact binary CI built; nothing is recompiled on the user's machine.
