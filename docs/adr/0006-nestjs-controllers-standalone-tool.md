# 0006 — NestJS-Style Controllers via Standalone Pre-Codegen Tool

**Status:** Accepted
**Date:** 2026-05-10
**Supersedes:** ADR-0005 (plugin approaches A and B rejected)

## Context

Users migrating from NestJS want ergonomic, decorator-driven routing where controllers declare their own routes via `@Controller`, `@Get`, `@Post`, etc., and are wired into an Express application without manually mapping each route. ADR-0005 explored three approaches. This document records the decision to implement **Approach C** — a standalone pre-codegen tool — and explains the design in full.

### Reframing the goal: minimal registration, not self-registration

NestJS itself is not zero-touch: every controller must be listed in a module's `controllers` array, and the root module must be passed to `NestFactory.create`. The ergonomic win NestJS delivers is *decentralized authorship* (the controller file declares its own path) combined with *one central registration point* (`AppModule`).

Anvil can match this. The acceptable target is:

- Each `@Controller`/`@Get`/`@Post` file declares its own routing metadata.
- The build tool reads those files and generates a single `routes.module.ts`.
- The user adds `RoutesModule` once, to their `@Component`'s `modules` list — equivalent to NestJS's `AppModule`.

That is minimal registration, not zero-touch self-registration. It satisfies the migration use case without requiring anvil's IR to grow plugin-specific concepts.

### Why the plugin approaches were rejected

ADR-0005's Approach A (Wasm plugin that writes `routes.module.ts`) and Approach B (two-phase plugin with opaque IR metadata) both require the generated `routes.module.ts` to be listed manually in `@Component`. Once that is acknowledged, the plugin's sole advantage over an external tool — staying inside anvil's build orchestration — disappears. At the same time, both plugin approaches carry costs that the standalone tool avoids:

- **Wasm ABI coupling.** Every time `PluginClassInfo`, `DecoratorArg`, or any related sidecar type evolves, the plugin must update or wait for a compatibility shim.
- **Approach B-specific: two-phase lifecycle.** Keeping plugin instances alive from the parse phase through codegen requires non-trivial changes to the CLI's plugin host and increases build-phase complexity.
- **No non-literal argument support.** Both plugin approaches run inside anvil's pipeline, which deliberately avoids invoking a type checker. Non-literal decorator arguments (identifiers, template literals, function calls) cannot be resolved and must be rejected. A standalone tool has the option of invoking `tsc` or ts-morph.

Given that the registration overhead is the same regardless, a standalone tool is strictly better: no ABI coupling, optional non-literal support, independent release cycle.

The existing `extract_bindings` plugin system (ADR-0003) is retained for other use cases — plugins that synthesize standard `@Inject`/`@Provides`-style bindings from third-party decorators. The NestJS route-generation use case does not fit that model because it produces a new TypeScript source file, not a set of bindings over existing types.

---

## Decision: `anvil-bellows`

A standalone CLI tool, published as `@anvil-di/bellows`, reads `@Controller`/`@Get`/`@Post`/`@Put`/`@Delete`/`@Patch` decorators from TypeScript source files and generates a `routes.module.ts` that anvil processes like any other user-written `@Module`.

### Inputs and outputs

**Input:** A directory (or glob) of TypeScript source files.

**Output:** A single `routes.module.ts` adjacent to the entry directory, containing:
- One `@Provides @IntoSet` static method per route handler, contributing to `Set<RouteDefinition>`.
- Import statements for each referenced controller class.
- A top-level `RoutesModule` class annotated with `@Module`.

The user adds `RoutesModule` to their `@Component`:

```typescript
@Component({ modules: [AppModule, RoutesModule] })
export abstract class AppComponent {
  abstract router(): Router;
}
```

This is the only manual step. Every subsequent change to a controller's decorator arguments is picked up automatically on the next run of `anvil-bellows`.

### Generated file shape

Given user-written controllers:

```typescript
// user-controller.ts
@Controller('/users')
@Inject
export class UserController {
  constructor(private repo: UserRepository) {}

  @Get('/:id')
  byId(req: Request, res: Response): void { ... }

  @Post('/')
  create(req: Request, res: Response): void { ... }
}

// health-controller.ts
@Controller('/health')
@Inject
export class HealthController {
  @Get('/')
  check(req: Request, res: Response): void { ... }
}
```

The tool generates:

```typescript
// routes.module.ts  (generated — do not edit)
import { Module, Provides, IntoSet } from "@msulak/anvil";
import { UserController } from "./user-controller";
import { HealthController } from "./health-controller";
import type { RouteDefinition } from "@anvil-di/bellows";

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

  @IntoSet @Provides
  static userControllerCreate(controller: UserController): RouteDefinition {
    return {
      method: "POST",
      path: "/users",
      handler: (req, res) => controller.create(req, res),
    };
  }

  @IntoSet @Provides
  static healthControllerCheck(controller: HealthController): RouteDefinition {
    return {
      method: "GET",
      path: "/health",
      handler: (req, res) => controller.check(req, res),
    };
  }
}
```

### `RouteDefinition` type

`RouteDefinition` is defined in `@anvil-di/bellows` (the companion runtime package, separate from the codegen tool):

```typescript
export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  handler: (req: express.Request, res: express.Response) => void | Promise<void>;
}
```

It is a dev dependency and type-only import in `routes.module.ts` — the `import type` ensures no runtime artifact is added by the generated file itself.

### App-side wiring

The user writes a `Router` class that consumes the multibinding:

```typescript
@Inject
export class Router {
  constructor(private routes: Set<RouteDefinition>) {}

  register(app: express.Application): void {
    for (const route of this.routes) {
      app[route.method.toLowerCase()](route.path, route.handler);
    }
  }
}
```

`Router` depends on `Set<RouteDefinition>`, which anvil resolves to the `@IntoSet` contributions in `RoutesModule`. This is identical to any other `@IntoSet` multibinding — the tool just automates the authorship of the contributing module.

---

## Non-literal decorator arguments

The tool parses decorator arguments with its own TypeScript AST pass. Arguments that are string, number, or boolean literals are accepted directly. Arguments that are not literals (identifier references, template literals, function calls, member expressions) require a decision:

**Default mode (static-only):** The tool emits a clear diagnostic and skips the affected handler:

```
Error: @Controller argument in user-controller.ts:4 is not a string literal.
  Found: BASE
  anvil-bellows cannot statically resolve identifier references.
  Use a string literal: @Controller('/users')
```

**`--tsc` mode (optional):** The tool invokes the TypeScript compiler API (`ts.createProgram`) to resolve the value of non-literal arguments. This is opt-in because it adds several seconds to cold builds on large projects. With `--tsc`:

```typescript
const BASE = '/users';

@Controller(BASE)   // resolved to '/users' via the compiler
```

The `--tsc` flag requires a `tsconfig.json` (auto-discovered from the entry directory, or specified with `--tsconfig`). Arguments that remain unresolvable after type-checking (e.g., runtime-computed values) still emit a diagnostic and are skipped.

This is the concrete advantage over plugin Approaches A and B: they run inside anvil's pipeline and cannot invoke a type checker; `anvil-bellows` is its own process and can.

---

## Build ordering and watch mode

### Without bundler integration

The user chains the commands:

```json
{
  "scripts": {
    "build": "anvil-bellows --entry src/ && anvil build --entry src/app.ts && tsc --noEmit"
  }
}
```

If `anvil-bellows` is not run first, anvil will produce a `MissingBinding` diagnostic for `Set<RouteDefinition>` (because `RoutesModule` exists in `@Component.modules` but hasn't been generated yet). This diagnostic is actionable — it names the missing key — but the error message is less precise than a "you forgot to run codegen" prompt. The tool should include instructions in its own README for the correct invocation order.

### With `anvil-unplugin` (`preBuild` hook)

The `anvil-unplugin` package already owns build orchestration in bundler contexts. A `preBuild` hook API lets tools like `anvil-bellows` run as a first-class step before `anvil build`, with ordering and re-run logic handled automatically:

```typescript
// vite.config.ts
import anvil from '@msulak/anvil-unplugin/vite';
import { bellowsCodegen } from '@anvil-di/bellows/codegen';

export default {
  plugins: [
    anvil({
      entry: 'src/app.ts',
      preBuild: [
        bellowsCodegen({ entry: 'src/', tsconfig: 'tsconfig.json' }),
      ],
    }),
  ],
};
```

`anvil-unplugin` runs `preBuild` hooks in order before invoking `anvil build`, so ordering is enforced automatically. In watch mode, `anvil-unplugin` receives file-change events from the bundler's native watcher. It re-runs `preBuild` hooks when any `@Controller`-bearing file changes (based on the hook's declared `watchPatterns`), then triggers a normal anvil rebuild. This eliminates the two-watcher composition problem for bundler-based projects.

```typescript
// The hook object returned by bellowsCodegen(...)
export interface PreBuildHook {
  name: string;
  watchPatterns: string[];   // e.g. ['src/**/*.ts']
  shouldRerun(changedFiles: string[]): boolean;
  run(): Promise<void>;
}
```

For CI and bare `package.json` scripts, the user chains commands manually. The `preBuild` integration is an ergonomic enhancement, not a hard requirement.

### Generated file status

`routes.module.ts` is both a codegen output and an input to the subsequent `anvil build` pass. This is the same status as hand-authored `@Module` files — anvil reads it and processes it normally. The file should be:

- **Committed to the repository.** Committing it makes the build reproducible without running `anvil-bellows` first, gives reviewers visibility into what routes exist, and keeps CI simple.
- **Marked generated at the top** (`// generated — do not edit`) to discourage manual edits that would be overwritten on the next run.
- **Excluded from `tsc` compilation** if and only if the project uses `tsconfig.json` `exclude`. In most projects it is fine to compile: it is valid TypeScript and `tsc` typechecks it against the actual controller method signatures, catching parameter mismatches early.

---

## Request validation

Validation has two sub-problems: *extraction* (which part of the request carries which data) and *schema enforcement* (whether that data is valid). Both are solved by the same mechanism: **parameter type annotations** on the controller method. The method signature is a complete, machine-readable description of the HTTP contract.

### Why NestJS-style parameter decorators are unavailable

NestJS's `@Param('id') id: string` syntax uses *parameter decorators* — decorators applied to individual constructor or method parameters. Parameter decorators are not part of the TC39 Stage 3 decorator proposal; they were a TypeScript-specific extension to the legacy `experimentalDecorators` system. ADR-0002 rules out `experimentalDecorators`. Extraction and validation information is therefore carried by *parameter type annotations*, not by decorators on the parameters themselves.

### Type-driven adapter generation (v0.2)

The tool reads each parameter's type annotation and generates the appropriate extraction, validation, and injection code. All schema-bearing types require `--tsc` mode because the schema references are runtime values that must be resolved to their source files.

#### Input wrapper types

Four parameter types are recognised. The first three carry a schema; the last two are raw passthrough:

| Parameter type | Extraction source | Behaviour |
|---|---|---|
| `Body<typeof Schema>` | `req.body` | Validates with `Schema.safeParse(req.body)`; 400 on failure |
| `Query<typeof Schema>` | `req.query` | Validates with `Schema.safeParse(req.query)`; 400 on failure |
| `Params<typeof Schema>` | `req.params` | Validates with `Schema.safeParse(req.params)`; 400 on failure |
| `express.Request` | — | Injects `req` directly; no validation |
| `express.Response` | — | Injects `res` directly; no validation |

`express.Request` and `express.Response` are not special cases — they are just two more types in the same dispatch table. A method that needs raw access to `req` or `res` (streaming, custom status codes, SSE) declares them as parameters alongside any schema-typed inputs.

All five types are exported from `@anvil-di/bellows`. The schema-bearing types are structurally defined against a `Validator<T>` interface so any schema library works:

```typescript
export interface Validator<T> {
  safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown };
}

export type Body<S extends Validator<unknown>>  = S extends Validator<infer T> ? T : never;
export type Query<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Params<S extends Validator<unknown>>= S extends Validator<infer T> ? T : never;
```

Zod schemas satisfy `Validator<T>` structurally; Valibot, ArkType, and class-validator wrappers work without changes to the tool.

#### The `Responds<T>` return type

The return type mirrors the parameter types. `Responds<typeof Schema>` declares that the method's return value is the response body and should be validated against `Schema` before being sent:

```typescript
export type Responds<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
```

If the return type is `Responds<T>` (or `Promise<Responds<T>>` for async methods), the adapter:
1. `await`s the result if the return type is wrapped in `Promise`
2. Calls `Schema.safeParse(result)`; a schema mismatch is a 500 (the controller returned something it shouldn't have)
3. Calls `res.json(result.data)` on success

If the return type is `void` or `Promise<void>`, the adapter does not call `res.json()` — the method is expected to handle `res` itself (it must have declared `res: express.Response` as a parameter to do so).

`Response<T>` was considered and rejected as the name: `express.Response` is already a valid parameter type in the same files, and having two distinct `Response` symbols — one for the raw Express object and one for the schema wrapper — would require aliasing at every import site. `Responds<T>` is unambiguous.

#### Async methods

TypeScript enforces that an `async` method must return `Promise<T>`. Users declare `Promise<Responds<typeof Schema>>` for async handlers; the tool detects the `Promise` wrapper, unwraps it to reach the schema (the same `promise_unwrap` pattern anvil-parser uses for `@Provides` methods in M12), and emits `await` in the adapter. Writing `async` but declaring `Responds<T>` without the `Promise` wrapper is a TypeScript error before the tool ever runs.

#### A complete example

```typescript
// user-controller.ts
import { Body, Params, Query, Responds } from '@anvil-di/bellows';
import { GetByIdParams, GetByIdQuery, GetByIdResponse, CreateUserBody } from './schemas';

@Controller('/users')
@Inject
export class UserController {
  constructor(private repo: UserRepository) {}

  @Get('/:id')
  async byId(
    params: Params<typeof GetByIdParams>,
    query:  Query<typeof GetByIdQuery>,
  ): Promise<Responds<typeof GetByIdResponse>> {
    return this.repo.findById(params.id, query);
  }

  @Post('/')
  async create(
    body: Body<typeof CreateUserBody>,
    res:  express.Response,        // needs custom status code
  ): Promise<void> {
    const user = await this.repo.create(body);
    res.status(201).json(user);
  }
}
```

Generated adapter for `byId`:

```typescript
handler: async (req, res) => {
  const paramsResult = GetByIdParams.safeParse(req.params);
  if (!paramsResult.success) { res.status(400).json(paramsResult.error); return; }
  const queryResult = GetByIdQuery.safeParse(req.query);
  if (!queryResult.success) { res.status(400).json(queryResult.error); return; }
  const result = await controller.byId(paramsResult.data, queryResult.data);
  const responseResult = GetByIdResponse.safeParse(result);
  if (!responseResult.success) { res.status(500).json({ error: 'Response validation failed' }); return; }
  res.json(responseResult.data);
},
```

Generated adapter for `create` (method manages `res` — no `Responds<T>`, raw `res` injected):

```typescript
handler: async (req, res) => {
  const bodyResult = CreateUserBody.safeParse(req.body);
  if (!bodyResult.success) { res.status(400).json(bodyResult.error); return; }
  await controller.create(bodyResult.data, res);
},
```

#### The contract the method signature encodes

Because every input and the output carry explicit schema references, the method signature is a complete description of the HTTP contract: what the endpoint accepts, from where, and what it returns. This is the foundation for OpenAPI spec generation and end-to-end type-safe clients without any additional annotation layer. A future `anvil-bellows-openapi` tool could derive a full OpenAPI document from `routes.module.ts` and the method signatures alone.

### Phase 1 (v0.1): `@Middleware` passthrough

Before v0.2 ships the type-driven adapter, `@Middleware` is the v0.1 mechanism. It also remains useful in v0.2 for cross-cutting concerns that operate outside the request/response schema (authentication, rate limiting, body parsing):

```typescript
@Controller('/admin')
@Middleware(requireAuth)       // class-level: applies to every method
@Inject
export class AdminController {
  @Delete('/:id')
  @Middleware(requireSuperAdmin)  // method-level: additional restriction
  async delete(params: Params<typeof DeleteParams>): Promise<Responds<typeof DeleteResponse>> { ... }
}
```

`@Middleware` arguments are non-literals (identifier references) and require `--tsc` mode. `@Middleware` runs before schema validation in the generated handler chain.

### Comparison

| | Requires `--tsc` | Validation library | Response validation | OpenAPI-ready |
|---|---|---|---|---|
| `@Middleware(fn)` | Yes | Any | No | No |
| `Body<T>` / `Query<T>` / `Params<T>` | Yes | Zod / structural adapter | — | Yes |
| `Responds<T>` | Yes | Zod / structural adapter | Yes | Yes |
| `express.Request` / `express.Response` | Yes (type resolution) | N/A | N/A | No |

Methods that use only `express.Request` / `express.Response` and no schema types could in principle avoid `--tsc`, since the tool only needs to detect the well-known type names. This optimisation is deferred; for v0.2 the rule is simple: any method with type-annotated parameters beyond plain `void` requires `--tsc`.

---

## Scope of the tool

### What `anvil-bellows` does

- Reads `@Controller`, `@Get`, `@Post`, `@Put`, `@Delete`, `@Patch` from `.ts` files in the target directory.
- Synthesizes a `@Module` with one `@IntoSet @Provides` static method per route handler.
- Writes `routes.module.ts` (default name, configurable).
- Emits actionable diagnostics for non-literal arguments (in static mode) or resolves them via `tsc` (with `--tsc`).
- **v0.1:** Wraps handlers in `@Middleware` chains when that decorator is present.
- **v0.2:** Generates full extraction, validation, and response-serialization adapters from `Body<T>`, `Query<T>`, `Params<T>`, `Responds<T>`, `express.Request`, and `express.Response` parameter/return type annotations (requires `--tsc`).

### What it does not do

- **Request-scoped injection.** Controllers that need per-request dependencies should use anvil's subcomponent pattern (M11). This is a v0.2+ feature.
- **Guards and interceptors.** NestJS's `@UseGuards`, `@UseInterceptors`, etc., are not handled. Cross-cutting concerns are expressed as Express middleware passed to `@Middleware`.
- **Module-scoped controller grouping.** All controllers contribute to a single `RoutesModule`. Splitting by subdirectory is a flag option for v0.2.
- **OpenAPI spec generation.** The method signatures contain enough information to derive a full OpenAPI document, but that is a separate tool (`anvil-bellows-openapi`) consuming `routes.module.ts` and the controller files. It is not part of `anvil-bellows`.

---

## Relationship to the existing plugin system

The `extract_bindings` Wasm plugin system (ADR-0003) is retained. It is the right tool for plugins that synthesize standard `@Inject`/`@Provides`-style bindings — for example, a plugin that reads `@Repository` decorators from a TypeORM-style codebase and emits `@Inject` bindings for each entity repository. The NestJS codegen use case does not fit that model:

- The output is a new TypeScript file, not a set of bindings over existing types.
- The tool is most useful as a one-time migration aid or a continuously regenerated file, both of which fit the standalone-tool model better than a plugin lifecycle.
- The plugin ABI is not yet stable; building a first-party feature on it would force a coordination commitment between the plugin SDK and the NestJS tool's release cycle.

ADR-0005 should be read as background context for the decision. The plugin approaches documented there remain available to third-party plugin authors who need to synthesize bindings from custom decorators. They are not the chosen implementation for the NestJS use case.

---

## Consequences

**Good:**
- No changes to `anvil-core` IR, `anvil-parser`, or `anvil-codegen`. The tool is a pure consumer of existing anvil conventions.
- Independent release cycle: the tool can ship fixes and new HTTP method support without coordinating with anvil's core release.
- Optional `--tsc` mode allows resolving non-literal decorator arguments and schema references — a capability that is structurally impossible for plugin approaches.
- Generated file is committed, inspectable, and typechecked by the user's own `tsc`.
- `preBuild` hook integration in `anvil-unplugin` recovers automatic ordering and watch mode for bundler-based projects.
- Controller methods become pure (or async-pure) functions when they use only schema-typed parameters and `Responds<T>`. No `req`/`res` in the method body means no mock objects needed in unit tests.
- The method signature is a complete HTTP contract: inputs with extraction sources and schemas, output with schema. This is sufficient for OpenAPI spec generation by a downstream tool without any additional annotations.

**Bad:**
- Bare `anvil build` scripts require the user to chain `anvil-bellows` first. Forgetting produces a `MissingBinding` diagnostic rather than a "run codegen first" prompt. The README must make this clear.
- `routes.module.ts` is a generated file that lives alongside user code. It requires discipline to avoid committing manual edits that will be overwritten. The `// generated — do not edit` banner and `.gitattributes` help but do not prevent accidents.
- Per-request injection (request-scoped controller deps) requires the subcomponent pattern rather than a NestJS-style `REQUEST` scope token. Users with deep per-request DI trees will need to refactor beyond simple controller decoration.
- All schema-typed parameter and return types require `--tsc` mode. Projects that avoid `--tsc` for build-time reasons must keep controllers in v0.1 form (`@Middleware` passthrough, raw `req`/`res`) and apply validation outside the generated adapter.
- Complex response shapes (multiple status codes, streaming, SSE) require declaring `res: express.Response` and handling the response manually — the `Responds<T>` single-return model only covers the common `200 OK` + JSON body case.

## Alternatives considered

- **Approach A (Wasm plugin, generates `routes.module.ts`):** Equivalent output, but requires the Wasm ABI and sidecar types, forces the plugin to stay in sync with anvil's release cycle, and provides no advantage over the standalone tool given that minimal registration is the goal. Rejected.
- **Approach B (two-phase plugin with opaque IR metadata):** Self-registration was its sole advantage over Approaches A and C. With minimal registration as the goal, self-registration is out of scope, and Approach B's two-phase lifecycle overhead is unwarranted. Rejected.
- **`@Module`/`@Provides` hand-authoring:** Users can write `RoutesModule` by hand without any tool. This is the fallback for teams that want full control or have non-literal routing arguments that the tool can't resolve even with `--tsc`. Documenting the hand-authored pattern is worthwhile in the migration guide.
- **Parameter decorators (`@Param('id') id: string`):** Not available — TC39 Stage 3 decorators do not include parameter decorators. Parameter *type annotations* (`params: Params<typeof Schema>`) are the available analogue and carry more information (the full schema, not just a field name).
- **Parameter names as extraction convention (`body`, `query`, `params`):** Using the identifier name of a parameter to determine its extraction source is fragile — a user writing `q` instead of `query` would silently misdirect extraction with no diagnostic. Dedicated wrapper types (`Body<T>`, `Query<T>`, `Params<T>`) encode the extraction source in the type and are checked by the TypeScript compiler.
- **`Response<T>` as the return-type wrapper:** `express.Response` is already a valid parameter type in controller files. Having two distinct things named `Response` in the same import namespace requires aliasing at every call site. `Responds<T>` is unambiguous.
- **Type-driven schema generation from plain TS types:** Inferring a Zod schema from `TypedRequest<{ params: { id: string } }>` is lossy — plain TS types cannot encode `z.string().uuid()` or `z.number().int().min(1)`. Users who need constraint-level precision write explicit schema objects and reference them via `Params<typeof Schema>`. The verbose form is intentional.
- **Global Express validation middleware:** Always available as a fallback for teams that avoid `--tsc` or want validation outside the generated adapter (`app.use(bodyValidator)`). Requires no tool support.
