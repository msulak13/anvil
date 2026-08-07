# `@anvil-di/anvil-unplugin`

Run [`@anvil-di/anvil`](https://github.com/msulak13/tsdi) codegen as part of your bundler's normal pipeline. Built on [`unplugin`](https://github.com/unjs/unplugin) so a single adapter works across Vite, Rollup, Webpack, Rspack, and esbuild.

## Why

Today's flow without the plugin:

```bash
anvil build        # regenerate every .anvil.ts
vite build         # bundle
```

That gets stale fast in dev — every edit to a `@Module` requires re-running `anvil build` before `vite dev`'s HMR will see the change. With the plugin:

```ts
// vite.config.ts
import anvil from "@anvil-di/anvil-unplugin/vite";

export default defineConfig({
  plugins: [anvil()],
});
```

Codegen runs on `buildStart` and on every relevant file change in dev (debounced). Validation errors flow through the bundler's normal error pipeline — Vite's overlay, Rollup's warning channel, Webpack's stats output.

## Per-bundler imports

```ts
import anvil from "@anvil-di/anvil-unplugin/vite";      // Vite plugin
import anvil from "@anvil-di/anvil-unplugin/rollup";    // Rollup plugin
import anvil from "@anvil-di/anvil-unplugin/webpack";   // Webpack plugin
import anvil from "@anvil-di/anvil-unplugin/rspack";    // Rspack plugin
import anvil from "@anvil-di/anvil-unplugin/esbuild";   // esbuild plugin
```

## Options

```ts
anvil({
  // Component entry files. Defaults to whatever anvil.config.json
  // declares, just like running `anvil build` with no args.
  entries: ["src/**/*-component.ts"],

  // Forwarded to `anvil build --tsconfig ...`. Lets the resolver honor
  // tsconfig `paths` / `baseUrl` aliases.
  tsconfig: "./tsconfig.json",

  // Path to the `anvil` binary. Defaults to the native binary resolved
  // via @anvil-di/anvil-cli's optionalDependencies.
  cli: "/path/to/anvil",

  // Debounce window for re-running codegen after a file edit (ms).
  // Defaults to 100, matching `anvil watch`.
  debounceMs: 100,

  // Which codegen backend to use:
  // - "native" (default): spawn the anvil Rust binary
  // - "wasm": run in-process via @anvil-di/anvil-codegen-wasm (no spawn overhead)
  mode: "native",

  // Delete anvil's no-op decorators before the bundler transforms them.
  // Defaults to true; see "Decorator stripping" below.
  stripDecorators: true,
});
```

## Decorator stripping

anvil is standard-decorators-only, but oxc — which Vite 6+, Rolldown, and Rspack transform with — implements only the *legacy* convention. Left alone, a standard decorator either reaches the output verbatim (nothing can parse it) or is applied with the wrong calling convention (which corrupts the class).

Because anvil's decorators do nothing at runtime, the plugin deletes them before the bundler sees one. This is on by default, and it is what lets an anvil project bundle correctly on an oxc-based toolchain at all.

```ts
// Also strip your own compile-time-only markers.
anvil({
  stripDecorators: { additionalModules: [/(?:^|\/)http\/public-route(?:\.js)?$/] },
});

// Or turn it off, if something else in your pipeline already handles
// standard decorators correctly (a ts-loader / tsc pre-pass, say).
anvil({ stripDecorators: false });
```

Only decorators resolving to a known no-op module are removed — one from any other package is left in place. See [`@anvil-di/strip-decorators`](../strip-decorators/README.md), which can also be used on its own if you don't need the codegen hooks.

## How it works

The plugin invokes the `anvil` CLI as a child process (native mode) or runs the WASM pipeline in-process (wasm mode):

- **`buildStart`** — runs once at the beginning of every build, blocking the bundler until codegen finishes. If `anvil build` fails, the bundler's error handling surfaces the diagnostic.
- **`watchChange`** — fires on every source `.ts` / `.tsx` edit in dev. Schedules a debounced rebuild; concurrent edits collapse into a single re-run. `.anvil.ts` outputs are ignored to avoid self-trigger loops.

The bundler's normal TypeScript pipeline then picks up the freshly generated `*.anvil.ts` files. There's no special TS plugin; `.anvil.ts` is just `.ts`.

## How the binary gets there

`@anvil-di/anvil-unplugin` depends on [`@anvil-di/anvil-cli`](../anvil-cli/README.md), which is the npm launcher for anvil's native Rust binary. `@anvil-di/anvil-cli` declares one `optionalDependencies` entry per supported platform (`@anvil-di/anvil-cli-linux-x64`, `@anvil-di/anvil-cli-darwin-arm64`, etc.); npm installs **only** the matching one based on `os`/`cpu` filters. At runtime the unplugin calls `resolveBinaryPath()` from `@anvil-di/anvil-cli` to find the binary — no PATH manipulation or `cargo install` required.

The same `cli` option still works if you want to point at a custom build (a `cargo build --release`'d binary outside `node_modules`, for example).

## Limitations (v0.2)

- In native mode the plugin spawns one process per build. Hot reload is fast (~50–200ms for a typical project); use `mode: "wasm"` for in-process codegen without the spawn overhead.
- Diagnostics flow as raw `anvil` stderr through the bundler's error API. Future work: parse the structured output and emit per-file diagnostics with source maps.
- Watch granularity is "any `.ts` change re-runs codegen for all configured entries". The CLI's M5 watch mode does smarter per-entry source-closure tracking; future versions of this plugin should reuse that data instead of duplicating its own filter.
