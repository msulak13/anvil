# `tsdi-unplugin`

Run [`tsdi`](https://github.com/msulak13/tsdi) codegen as part of your bundler's normal pipeline. Built on [`unplugin`](https://github.com/unjs/unplugin) so a single adapter works across Vite, Rollup, Webpack, Rspack, and esbuild.

## Why

Today's flow without the plugin:

```bash
tsdi build         # regenerate every .tsdi.ts
vite build         # bundle
```

That gets stale fast in dev — every edit to a `@Module` requires re-running `tsdi build` before `vite dev`'s HMR will see the change. With the plugin:

```ts
// vite.config.ts
import tsdi from "tsdi-unplugin/vite";

export default defineConfig({
  plugins: [tsdi()],
});
```

Codegen runs on `buildStart` and on every relevant file change in dev (debounced). Validation errors flow through the bundler's normal error pipeline — Vite's overlay, Rollup's warning channel, Webpack's stats output.

## Per-bundler imports

```ts
import tsdi from "tsdi-unplugin/vite";      // Vite plugin
import tsdi from "tsdi-unplugin/rollup";    // Rollup plugin
import tsdi from "tsdi-unplugin/webpack";   // Webpack plugin
import tsdi from "tsdi-unplugin/rspack";    // Rspack plugin
import tsdi from "tsdi-unplugin/esbuild";   // esbuild plugin
```

## Options

```ts
tsdi({
  // Component entry files. Defaults to whatever tsdi.config.json
  // declares, just like running `tsdi build` with no args.
  entries: ["src/**/*-component.ts"],

  // Forwarded to `tsdi build --tsconfig ...`. Lets the resolver honor
  // tsconfig `paths` / `baseUrl` aliases.
  tsconfig: "./tsconfig.json",

  // Path to (or name of) the `tsdi` binary. Defaults to "tsdi" — the
  // npm package's bin, or anything on $PATH.
  cli: "tsdi",

  // Debounce window for re-running codegen after a file edit (ms).
  // Defaults to 100, matching `tsdi watch`.
  debounceMs: 100,
});
```

## How it works

The plugin invokes the `tsdi` CLI as a child process:

- **`buildStart`** — runs once at the beginning of every build, blocking the bundler until codegen finishes. If `tsdi build` fails, the bundler's error handling surfaces the diagnostic.
- **`watchChange`** — fires on every source `.ts` / `.tsx` edit in dev. Schedules a debounced rebuild; concurrent edits collapse into a single re-run. `.tsdi.ts` outputs are ignored to avoid self-trigger loops.

The bundler's normal TypeScript pipeline then picks up the freshly generated `*.tsdi.ts` files. There's no special TS plugin; `.tsdi.ts` is just `.ts`.

## Limitations (v0.2)

- The plugin shells out to the native `tsdi` Rust binary. You need that binary installed (today: `cargo install` from the workspace, or use the published npm wrapper once it ships). A pure-WASM build of `tsdi-codegen` is on the roadmap — that would let `tsdi-unplugin` stand alone with no native deps.
- Diagnostics flow as raw `tsdi` stderr through the bundler's error API. Future work: parse the structured `tsdi-cli` output and emit per-file diagnostics with source maps.
- Watch granularity is "any `.ts` change re-runs codegen for all configured entries". The CLI's M5 watch mode does smarter per-entry source-closure tracking; future versions of this plugin should reuse that data instead of duplicating its own filter.
