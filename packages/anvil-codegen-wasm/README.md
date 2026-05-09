# `@msulak/anvil-codegen-wasm`

WASM build of the [`@msulak/anvil`](https://github.com/msulak13/tsdi) codegen pipeline. Same parser, graph builder, validator, and emitter as the native CLI — exactly the Rust source, compiled with `wasm-pack` — running in-process for any host that runs WebAssembly (Node, browsers, Workers, Bun, Deno, StackBlitz).

## Why

The native [`@msulak/anvil-cli`](../anvil-cli/README.md) is fast, but every invocation still pays a process-spawn cost (~10ms). Inside a bundler's hot path, that's the difference between "imperceptible" and "every save has a noticeable hitch". `@msulak/anvil-codegen-wasm` is a direct function call:

```ts
import { compile } from "@msulak/anvil-codegen-wasm";

const result = compile({
  entryPath: "/abs/src/app-component.ts",
  files: { "/abs/src/app-component.ts": "...", "/abs/src/heater.ts": "..." },
  version: "0.0.1",
});
```

No spawn, no IPC, no file write — `result.emittedFiles` is the dagger output as in-memory strings, ready to feed straight into Vite's transform pipeline or write to disk if you prefer.

## API

```ts
interface CompileInput {
  entryPath: string;                  // absolute path to the @Component
  files: Record<string, string>;      // every reachable file, abs path → source
  aliases?: PathAlias[];              // tsconfig `paths` aliases
  version: string;                    // baked into the dagger banner comment
}

interface PathAlias {
  pattern: string;   // "@/*"
  target: string;    // "src/*"
  baseDir: string;   // absolute dir the relative target resolves against
}

interface CompileOutput {
  emittedFiles: { path: string; contents: string }[];
  diagnostics: Diagnostic[];          // structured; route to bundler error UI
}
```

Diagnostics are **non-fatal** — the function returns normally even when validation fails. Hosts decide how to render them (Vite overlay, Rollup `this.warn`, Webpack stats output, plain `console.error`, etc.).

## How it gets built

The Rust source lives in `crates/anvil-codegen-wasm/`. `wasm-pack build --target nodejs` produces:

- `dist/anvil_codegen_wasm_bg.wasm` — the WASM blob (~1.4MB unoptimized; gzips to ~400KB)
- `dist/anvil_codegen_wasm.js` — the wasm-bindgen JS glue
- `lib/index.js` — typed ESM wrapper (this package)

The blob includes the entire pipeline: `oxc_parser`, `oxc_codegen`, `anvil-core` (graph + validation), `anvil-parser` (decorator extraction), and `anvil-codegen` (TS emission). The only piece that's *different* from the native CLI is the resolver — the WASM build uses a small custom in-memory resolver instead of `oxc_resolver`'s full filesystem-backed implementation. Tsconfig `paths` aliases work; the rest of `oxc_resolver`'s feature set (browser fields, conditional exports, workspace overrides) is the bundler's job.

## Path conventions

The wasm32 Rust target uses Unix-style path semantics — backslashes are part of filenames, not separators. The WASM compile boundary normalizes Windows-style paths (`C:\Users\foo`) to forward-slash form (`C:/Users/foo`) automatically; the emitted file paths come back in the same form. Hosts running on Windows can convert back to backslashes for `fs.writeFileSync` calls. (The `@msulak/anvil-unplugin` does this transparently.)

## Output equivalence

Both the WASM and native paths emit **byte-identical output** for the same input — same parser, same graph builder, same emitter, same `oxc_codegen` canonicalization. The golden-fixture suite under `tests/fixtures/` validates the native path; the WASM path is unit-tested for the same shapes (parser → graph → emit) in this package's `src/index.test.ts`.

## Tradeoffs vs the native binary

| Property | Native | WASM |
|---|---|---|
| Cold start | ~10ms (process spawn) | ~80-150ms (wasm compile + instantiate) |
| Steady-state codegen | 1× | ~1.5-2× slower (oxc parsing is the bottleneck) |
| Bundler hot path | spawn + IPC per build | direct function call |
| Install footprint | one of five platform packages (~4MB) | single ~1.4MB .wasm |
| Reach | linux/macos/windows on x64+arm64 | anywhere V8/wasmtime runs |

Both paths are first-class. `@msulak/anvil-unplugin` defaults to native (faster) and accepts `mode: "wasm"` as an in-process opt-in.
