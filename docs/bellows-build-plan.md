# Bellows Build Plan

Implementation plan for `anvil-bellows` (ADR-0006) and `anvil-bellows-openapi` (ADR-0007). Five milestones across three new packages.

## Package map

| Package | npm name | Role |
|---|---|---|
| `packages/anvil-bellows` | `@msulak/anvil-bellows` | Runtime types, decorator stubs, schema utilities |
| `packages/anvil-bellows-codegen` | `@msulak/anvil-bellows` (bin: `anvil-bellows`) | Controller → `routes.module.ts` codegen CLI |
| `packages/anvil-bellows-openapi` | `@msulak/anvil-bellows-openapi` | Controller → OpenAPI 3.1 document CLI |

The runtime package and the codegen CLI share the same npm name because the runtime types (`Body<T>`, `Responds<T>`, etc.) are what users import in their controller files, and the CLI binary is what they invoke in build scripts — bundling them avoids a second dependency declaration. They are separate directories in the monorepo for clean separation of concerns.

## Dependency graph

```
M1 (runtime) ──► M2 (static codegen) ──► M3 (--tsc codegen)
                                               │
                 M4 (unplugin hooks) ◄─────────┤
                                               │
                                               └──► M5 (openapi)
```

M1 and M4 have no prerequisites. M2 requires M1. M3 extends M2. M5 requires M3 (schema resolution) and M4 (postBuild hook), but M5's core logic can be developed in parallel with M4 once M3 is done.

---

## M1 — `@msulak/anvil-bellows` runtime package

**New package:** `packages/anvil-bellows`

### Deliverables

**Decorator stubs** — Stage 3, all no-ops at runtime:

```typescript
export declare const Controller: (path: string) => ClassDecorator;
export declare const Get:        (path: string) => MethodDecorator;
export declare const Post:       (path: string) => MethodDecorator;
export declare const Put:        (path: string) => MethodDecorator;
export declare const Delete:     (path: string) => MethodDecorator;
export declare const Patch:      (path: string) => MethodDecorator;
export declare const Middleware: (...fns: ExpressMiddleware[]) => ClassDecorator & MethodDecorator;
export declare const Tag:        (name: string) => ClassDecorator;
export declare const Returns:    (status: number, schema: unknown) => MethodDecorator;
export declare const Security:   (scheme: string) => ClassDecorator & MethodDecorator;
export declare const Deprecated: (reason?: string) => MethodDecorator;
```

**Schema interface and wrapper types:**

```typescript
export interface Validator<T> {
  safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown };
  jsonSchema?(): JSONSchema7;   // optional; used by anvil-bellows-openapi
}

export type Body<S extends Validator<unknown>>    = S extends Validator<infer T> ? T : never;
export type Query<S extends Validator<unknown>>   = S extends Validator<infer T> ? T : never;
export type Params<S extends Validator<unknown>>  = S extends Validator<infer T> ? T : never;
export type Responds<S extends Validator<unknown>>= S extends Validator<infer T> ? T : never;
```

**Utility:**

```typescript
export function withJsonSchema<T>(
  validator: Validator<T>,
  schema: JSONSchema7,
): Validator<T> & { jsonSchema(): JSONSchema7 };
```

**Route types** (consumed by the app-side `Router` class):

```typescript
export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  handler: (req: express.Request, res: express.Response) => void | Promise<void>;
}
```

### Tests

- `Validator<T>` is satisfied structurally by a Zod schema (type-level test via `satisfies`)
- `withJsonSchema` round-trips: `wrapped.safeParse(x)` delegates to the original validator, `wrapped.jsonSchema()` returns the provided schema
- All decorator stubs return their target unchanged (no-op check)

### Notes

- `express` is a `peerDependency` (types only: `@types/express` as dev dep)
- `JSONSchema7` comes from `@types/json-schema`
- No build step required — pure TypeScript declarations, `tsc` only

---

## M2 — `anvil-bellows` static mode (v0.1)

**New package:** `packages/anvil-bellows-codegen`  
**Published as:** `@msulak/anvil-bellows` (adds `bin: { "anvil-bellows": "./dist/cli.js" }` to the runtime package's `package.json`, or as a separate package depending on size)

### Deliverables

**Parser** — TypeScript compiler API in `ts.createSourceFile` mode (no full program, no type resolution, fast cold start). Reads:
- `@Controller(literal)` on classes
- `@Get(literal)` / `@Post(literal)` / `@Put` / `@Delete` / `@Patch` on methods
- Class name, method names
- `@Middleware`, `@Tag` (recorded but not resolved in static mode)

**Generator** — Emits `routes.module.ts`:

```typescript
// routes.module.ts  (generated — do not edit)
import { Module, Provides, IntoSet } from "@msulak/anvil";
import { UserController } from "./user-controller";
import type { RouteDefinition } from "@msulak/anvil-bellows";

@Module
export class RoutesModule {
  @IntoSet @Provides
  static userControllerGetById(controller: UserController): RouteDefinition {
    return {
      method: "GET",
      path: "/users/:id",
      handler: (req, res) => controller.byId(req, res),
    };
  }
}
```

v0.1 adapter shape: `(req, res) => controller.method(req, res)`. Schema-typed parameters are not yet recognised.

**Diagnostics** — Non-literal `@Controller`/`@Get` arguments emit a clear error and skip the affected route:

```
Error [anvil-bellows]: @Controller argument is not a string literal.
  Found identifier: BASE
  Hint: use a string literal, or re-run with --tsc to resolve constants.
  → src/user-controller.ts:4
```

**CLI:**

```
anvil-bellows --entry <dir> [--output <file>] [--tsconfig <path>] [--tsc]
```

`--entry` defaults to `./src`. `--output` defaults to `<entry>/routes.module.ts`.

**`PreBuildHook` interface** (defined here, consumed by M4):

```typescript
export interface PreBuildHook {
  name: string;
  watchPatterns: string[];
  shouldRerun(changedFiles: string[]): boolean;
  run(): Promise<void>;
}

export function bellowsCodegen(options: {
  entry: string;
  output?: string;
  tsconfig?: string;
  tsc?: boolean;
}): PreBuildHook;
```

### Tests

- Fixture: two controller files with literal paths → assert generated `routes.module.ts` matches snapshot
- Non-literal `@Controller` arg → assert diagnostic emitted, route skipped
- Generated file passes `tsc --noEmit` against `packages/anvil` and `packages/anvil-bellows` runtime stubs
- `bellowsCodegen({ entry })` returns a hook with correct `watchPatterns` (globs the entry directory)

### Risk: `typeof Schema` extraction

The v0.2 adapter calls `GetByIdParams.safeParse(...)` — it needs the *expression* `GetByIdParams`, not the *type* `typeof GetByIdParams`. When the tool reads a parameter typed as `Body<typeof GetByIdParams>`, it must strip the `typeof` wrapper to extract the identifier. Getting this wrong silently emits `typeof GetByIdParams.safeParse(...)`, which is a type-level expression, not a value call — `tsc --noEmit` will catch it, but the failure message will be confusing. Add an explicit test asserting the generated adapter calls `GetByIdParams.safeParse(req.body)` (no `typeof`).

---

## M3 — `--tsc` mode and type-driven adapters (v0.2)

**Extends:** M2 (same package, `--tsc` flag switches parser mode)

### Deliverables

**Full-program mode** — `ts.createProgram` with the project's `tsconfig.json`. Enables:

- Resolving non-literal `@Controller`/`@Get` arguments (identifier references, `const` values)
- Resolving `@Middleware(fn)` to its source file; adding the import in `routes.module.ts`; wrapping the handler

**Type-driven adapter generation** — the tool inspects each parameter's type annotation:

| Detected type | Extraction | Generated code |
|---|---|---|
| `Body<typeof S>` | `req.body` | `const r = S.safeParse(req.body); if (!r.success) { res.status(400).json(r.error); return; }` |
| `Query<typeof S>` | `req.query` | Same, `req.query` |
| `Params<typeof S>` | `req.params` | Same, `req.params` |
| `express.Request` | — | Injects `req` directly |
| `express.Response` | — | Injects `res` directly |

**Return type handling:**

| Return type | Generated code |
|---|---|
| `Responds<typeof S>` | `const r = S.safeParse(result); if (!r.success) { res.status(500)...; return; } res.json(r.data);` |
| `Promise<Responds<typeof S>>` | Same but `await`s the controller call first |
| `void` / `Promise<void>` | No `res.json()` emitted; method manages `res` itself |

**`@Middleware` ordering** — when both `@Middleware` and schema parameters are present, `@Middleware` runs first in the generated chain.

### Tests

- Fixture: controller using all five parameter types + `Responds<T>` → assert generated adapter shape (snapshot)
- `Promise<Responds<T>>` return type → adapter emits `await` + response validation
- `void` return + `res: express.Response` parameter → no `res.json()` emitted
- `@Middleware(fn)` → fn imported and called before controller method
- Non-literal `@Controller` resolved via `--tsc` → correct path in generated file
- Generated file passes `tsc --noEmit` in all cases

### Risk: `express.Request` / `express.Response` identity

The tool must match these types by their *declaration module path* (resolves to `node_modules/@types/express/...`), not by the bare name `Request` or `Response`. A user-defined type named `Response` would otherwise be misidentified. Use the compiler API's `typeChecker.getSymbolAtLocation` + `symbol.declarations[0].getSourceFile().fileName` to verify the module. Add a test with a user-defined `Response` type that must *not* trigger passthrough injection.

---

## M4 — `anvil-unplugin` preBuild/postBuild hook API

**Extends:** `packages/anvil-unplugin`

### Deliverables

**Hook interfaces** (move `PreBuildHook` here from M2, add `PostBuildHook`):

```typescript
export interface PreBuildHook {
  name: string;
  watchPatterns: string[];
  shouldRerun(changedFiles: string[]): boolean;
  run(): Promise<void>;
}

export interface PostBuildHook {
  name: string;
  watchPatterns: string[];
  shouldRerun(changedFiles: string[]): boolean;
  run(): Promise<void>;
}
```

**Options extension:**

```typescript
interface AnvilPluginOptions {
  // existing fields ...
  preBuild?:  PreBuildHook[];
  postBuild?: PostBuildHook[];
}
```

**Execution order in `buildStart`:**

1. Run `preBuild` hooks in array order; fail fast on any error
2. Run `anvil build`
3. Run `postBuild` hooks in array order

**Watch mode** — on file change:

1. For each hook where `shouldRerun(changedFiles)` returns true, re-run it
2. If any pre-build hook re-ran, also re-run `anvil build` and all post-build hooks
3. If only post-build hooks re-ran, skip `anvil build`

**Usage:**

```typescript
// vite.config.ts
import anvil from '@msulak/anvil-unplugin/vite';
import { bellowsCodegen } from '@msulak/anvil-bellows';
import { bellowsOpenApi } from '@msulak/anvil-bellows-openapi/unplugin';

export default {
  plugins: [
    anvil({
      entry: 'src/app.ts',
      preBuild:  [bellowsCodegen({ entry: 'src/' })],
      postBuild: [bellowsOpenApi({ entry: 'src/', output: 'openapi.yaml' })],
    }),
  ],
};
```

### Tests

- Extend existing `anvil-unplugin` integration test: add a `preBuild` hook that writes a file, assert file exists after `buildStart`
- Watch mode: assert hook re-runs when a file matching `watchPatterns` changes, does not re-run on unrelated changes

---

## M5 — `anvil-bellows-openapi`

**New package:** `packages/anvil-bellows-openapi`

### Deliverables

**Shared parser library** — extract the controller-file parsing logic from M3 into an internal `packages/anvil-bellows-parser` (not published to npm). Both `anvil-bellows` and `anvil-bellows-openapi` depend on it. Avoids duplication and divergence. This extraction is part of M5 setup, not a separate milestone.

**OpenAPI document builder:**

- Derives paths, HTTP methods, operation IDs from `@Controller` + `@Get`/etc.
- `Params<typeof S>` → OpenAPI parameters with `in: "path"`, names from schema properties, requires `--tsc`
- `Query<typeof S>` → parameters with `in: "query"`
- `Body<typeof S>` → `requestBody` with `application/json` content
- `Responds<typeof S>` / `Promise<Responds<T>>` → `responses.200`
- `@Returns(status, typeof S)` → additional response objects, requires `--tsc`
- `@Security(scheme)` → operation-level security requirement (string literal, static mode)
- `@Tag(name)` → operation tags override (string literal, static mode); default: controller class name minus `Controller` suffix, lowercased
- `@Deprecated(reason)` → `deprecated: true` + reason appended to description
- JSDoc first line → `summary`; full body → `description` (requires `--tsc`)

**Schema conversion:**

- Zod auto-detection: check `schema._def`, call `zod-to-json-schema` (optional peer dependency; emit a warning if absent and a Zod schema is encountered)
- `jsonSchema()` method on `Validator<T>` (from M1): called if present
- `withJsonSchema()` wrapper schemas: call `jsonSchema()` directly

**Config file** (`anvil-bellows-openapi.config.yaml` or `.json`):

```yaml
info:
  title: My API
  version: 1.0.0
servers:
  - url: https://api.example.com
securitySchemes:
  bearerAuth:
    type: http
    scheme: bearer
```

`@Security` scheme names are validated against `securitySchemes`; a mismatch is a diagnostic (not a crash).

**Output validation** — generated document is validated against the OpenAPI 3.1 meta-schema before being written to disk. A schema violation is a tool error with the failing path printed.

**CLI:**

```
anvil-bellows-openapi --entry <dir> --output <file> [--format yaml|json] [--config <path>] [--tsc] [--tsconfig <path>]
```

**`PostBuildHook` export:**

```typescript
import { bellowsOpenApi } from '@msulak/anvil-bellows-openapi/unplugin';
```

### Tests

- Fixture: controllers with `Body<T>`, `Query<T>`, `Params<T>`, `Responds<T>` → assert generated OpenAPI document structure (snapshot)
- `@Returns`, `@Security`, `@Deprecated`, `@Tag` → assert corresponding OpenAPI fields
- Zod schema round-trip: `z.object({ id: z.string().uuid() })` → assert `format: uuid` in the output schema
- Validate generated document against OpenAPI 3.1 meta-schema (use `@apidevtools/swagger-parser` or `ajv`)
- `@Security` mismatch (scheme not in config) → assert diagnostic emitted

### Risk: shared parser drift

If `anvil-bellows-parser` is not extracted and both tools copy the parsing logic, they will diverge silently when one is updated. The extraction adds ~half a day of setup but pays back immediately: `anvil-bellows-openapi` gets `--tsc` schema resolution for free from the shared library rather than reimplementing it.

---

## Testing layers summary

| Layer | What | When |
|---|---|---|
| Unit | Type-level correctness, `Validator<T>` satisfaction, `withJsonSchema` | M1 |
| Snapshot | Generated `routes.module.ts` shape | M2, M3 |
| Integration | `tsc --noEmit` against generated output | M2, M3 |
| Integration | OpenAPI document structure + meta-schema validation | M5 |
| E2E | Full `anvil-bellows → anvil build → tsc` pipeline in tempdir | M3 |
| Unplugin | preBuild/postBuild hook execution and watch mode | M4 |

---

## Open questions

1. **Single npm package vs two.** The runtime types (`Body<T>` etc.) and the CLI binary (`anvil-bellows`) currently share the `@msulak/anvil-bellows` package name. If the CLI grows heavy dependencies (TypeScript compiler, etc.) that shouldn't be in a user's `dependencies`, split into `@msulak/anvil-bellows` (runtime, lean) and `@msulak/anvil-bellows-cli` (codegen binary). Decide at M2 once the dependency list is known.

2. **`--tsc` requirement for `express.Request`/`express.Response` passthrough.** Methods that only use raw `req`/`res` and no schema types could in principle avoid `--tsc` (the tool just needs to recognise the well-known names). The current plan requires `--tsc` for any schema-type parameter; raw-only methods could be exempted as an optimisation. Defer until there is a reported pain point.

3. **Shared parser package publishing.** `anvil-bellows-parser` is listed as internal (not published). If third-party tools want to build on the same parsing logic, publish it. Defer until there is external demand.
