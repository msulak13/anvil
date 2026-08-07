# `@anvil-di/strip-decorators`

Remove anvil's and bellows' no-op decorators from your source before the bundler transforms it.

## Why this exists

anvil is [standard-decorators-only](../../docs/adr/0002-stage3-decorators-only.md) — TC39 Stage 3, never `experimentalDecorators`. oxc, which **Rolldown, Vite 6+, and Rspack** transform with, implements only the *legacy* convention. That leaves an anvil project with two broken options and no good one:

| oxc setting | what you get |
| --- | --- |
| default (`decorator.legacy: false`) | Decorators pass through into the output verbatim: `var C = @Controller("/x") class {...}`. No JS engine can parse that — the bundle throws `SyntaxError` on import. |
| `decorator.legacy: true` | Method decorators are called as `(target, key, descriptor)`, but anvil's are written `(target, context) => target`. `__decorate` then does `Object.defineProperty(target, key, theClass)`, corrupting the class. |

The usual workaround is to route the affected files through `tsc`, whose standard-decorator emit is correct. That works, but buys a dependency on TypeScript's **JS compiler API** — which TypeScript 7 (the native port) does not ship.

This package takes the third option: **delete the decorators**.

## Why deleting is correct

Every decorator in `@anvil-di/anvil` and `@anvil-di/bellows` is an identity function:

```ts
export function Inject<T extends abstract new (...args: never[]) => unknown>(
  target: T,
  _ctx: ClassDecoratorContext<T>,
): T {
  return target;          // ← that's the whole implementation
}
```

They exist so your code typechecks. The wiring they describe is read out of the **source** by the Rust toolchain during `anvil build` / `anvil-bellows`, before a bundler is ever involved. This is invariant #1 of the project: *"No runtime reflection. Decorators are no-ops at runtime; all semantics live in the Rust toolchain."*

So applying them is observably identical to not applying them — minus the `__esDecorate`/`__runInitializers` helpers a real emit inlines into every module that uses one.

## Safety

Only decorators whose binding resolves to a **known no-op module** are removed. Everything else is left exactly where it is:

```ts
import { Controller } from "@anvil-di/bellows";
import { observable } from "mobx";

@Controller("/x")            // ← removed
export class C {
  @observable count = 1;     // ← kept: not ours, might do something
}
```

A decorator with real behaviour therefore survives into the output and fails loudly, rather than being silently dropped. Name matching would not be safe enough — a local helper also called `Get` must not be touched — so resolution is by import binding, and aliases (`import { Inject as I }`) and namespace imports (`@anvil.Inject`) resolve correctly.

## Usage

### As a bundler plugin

```ts
// rolldown.config.ts
import stripDecorators from "@anvil-di/strip-decorators/rolldown";

export default {
  input: "src/index.ts",
  plugins: [stripDecorators()],
};
```

Entry points: `/rolldown`, `/rollup`, `/vite`, `/webpack`, `/rspack`, `/esbuild`.

If you already use [`@anvil-di/anvil-unplugin`](../anvil-unplugin), you get this for free — it runs the same transform by default, and you can turn it off with `stripDecorators: false`.

### Your own marker decorators

A project often has its own compile-time-only decorator that `anvil-bellows` reads and nothing executes. Declare it, and it is stripped too:

```ts
stripDecorators({
  additionalModules: [/(?:^|\/)http\/public-route(?:\.js)?$/],
});
```

A string matches a specifier exactly; a `RegExp` is tested against it. Relative specifiers are matched **as written in the importing file**, so prefer a `RegExp`.

> Adding a module here asserts that none of its decorators do anything at runtime. That claim is not verified.

### Directly

```ts
import { stripNoopDecorators, mayContainNoopDecorators } from "@anvil-di/strip-decorators";

if (mayContainNoopDecorators(code)) {
  const result = stripNoopDecorators(code, id);
  if (result !== null) {
    // result.code, result.map, result.removed
  }
}
```

`stripNoopDecorators` returns `null` when there is nothing to do, so callers can pass the original source and its existing source map straight through. `mayContainNoopDecorators` is a cheap pre-filter with no false negatives.

## What it costs

Nothing to speak of: one oxc parse of the files that actually import anvil, versus a full TypeScript transpile of the same files. On a real service with 54 decorated files, swapping `ts.transpileModule` for this cut the bundle from 21,720 to 13,884 lines — all of it dead decorator machinery — and the build got slightly faster.
