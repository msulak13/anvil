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

anvil is [standard-decorators-only](../../docs/adr/0002-stage3-decorators-only.md). oxc — which **Rolldown, Vite 6+, and Rspack** transform with — implements only the *legacy* convention, which leaves an anvil project with two broken options:

| oxc setting | result |
| --- | --- |
| default (`decorator.legacy: false`) | Decorators reach the output verbatim: `var C = @Controller("/x") class {...}`. Importing the bundle throws `SyntaxError`. |
| `decorator.legacy: true` | Method decorators are called `(target, key, descriptor)`, but anvil's are `(target, context) => target`. `__decorate` then does `Object.defineProperty(target, key, theClass)` and corrupts the class. |

The usual escape is routing those files through `tsc`, whose standard-decorator emit is correct — at the cost of a dependency on TypeScript's JS compiler API, which TypeScript 7 does not ship.

So the plugin takes a third option: it **deletes** the decorators before the bundler sees one. This is on by default, and it is what lets an anvil project bundle correctly on an oxc-based toolchain at all.

### Why deleting is correct

Every decorator in `@anvil-di/anvil` and `@anvil-di/bellows` is an identity function — `return target`. They exist so your code typechecks; the wiring they describe is read out of the **source** by the Rust toolchain during `anvil build` / `anvil-bellows`, before a bundler is involved. This is invariant #1: *"No runtime reflection. Decorators are no-ops at runtime."*

Applying them is therefore observably identical to not applying them, minus the `__esDecorate`/`__runInitializers` helpers a real emit inlines into every module that uses one.

### Safety

Only decorators whose binding resolves to a **known no-op module** are removed. Matching is by import binding, not by name, so a local helper that happens to be called `Get` is untouched; aliases (`import { Inject as I }`) and namespace imports (`@anvil.Inject`) resolve correctly.

```ts
@Controller("/x")          // removed
export class C {
  @observable count = 1;   // kept — not ours, might do something
}
```

A decorator with real behaviour therefore survives into the output and fails loudly, rather than being silently dropped.

### Options

```ts
// Also strip your own compile-time-only markers — a `@Public(reason)` that
// only anvil-bellows reads, say. A string matches a specifier exactly; a
// RegExp is tested against it. Relative specifiers match as written in the
// importing file, so prefer a RegExp.
anvil({
  stripDecorators: { additionalModules: [/(?:^|\/)http\/public-route(?:\.js)?$/] },
});

// Or turn it off, if something else in your pipeline already handles
// standard decorators correctly (a ts-loader / tsc pre-pass, say).
anvil({ stripDecorators: false });
```

> Adding a module to `additionalModules` asserts that none of its decorators do anything at runtime. That claim is not verified.

### Without the codegen hooks

A project that already runs `anvil build` / `anvil-bellows` its own way (a `gen` npm script, say) and only needs its bundle to come out runnable can take the transform alone:

```ts
// rolldown.config.ts
import { stripDecoratorsUnplugin } from "@anvil-di/anvil-unplugin/strip";

export default {
  input: "src/index.ts",
  plugins: [stripDecoratorsUnplugin.rolldown()],
};
```

`.vite()`, `.rollup()`, `.webpack()`, `.rspack()` and `.esbuild()` are available the same way. The underlying functions — `stripNoopDecorators(code, id, options)` and the cheap pre-filter `mayContainNoopDecorators(code, options)` — are exported from the same entry point for hand-written transforms. `stripNoopDecorators` returns `null` when there is nothing to do, so callers can pass the original source and its existing source map straight through.

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
