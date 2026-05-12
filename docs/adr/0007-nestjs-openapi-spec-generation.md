# 0007 — OpenAPI Spec Generation from Controller Method Signatures

**Status:** Proposed
**Date:** 2026-05-12
**Depends on:** ADR-0006 (`anvil-nestjs-codegen`, `Body<T>`, `Query<T>`, `Params<T>`, `Responds<T>`)

## Context

ADR-0006 establishes that controller method signatures are a complete, machine-readable HTTP contract:

```typescript
@Get('/:id')
async byId(
  params: Params<typeof GetByIdParams>,
  query:  Query<typeof GetByIdQuery>,
): Promise<Responds<typeof GetByIdResponse>>
```

This signature encodes path, method, parameter sources, schemas for every input, and the response schema — everything an OpenAPI operation object needs. No additional annotations are required for the common case. A separate tool can derive a full OpenAPI 3.1 document from the controller files without the user writing a single `@ApiProperty` or `@ApiOperation` decorator.

### What is and is not available without extra annotation

| OpenAPI field | Source | Available without annotation |
|---|---|---|
| Path | `@Controller` + `@Get`/`@Post`/… | Yes |
| HTTP method | `@Get`/`@Post`/… | Yes |
| Operation ID | Method name | Yes |
| Path parameters | `Params<typeof Schema>` — schema property names + types | Yes (requires `--tsc`) |
| Query parameters | `Query<typeof Schema>` — schema property names, types, optionality | Yes (requires `--tsc`) |
| Request body schema | `Body<typeof Schema>` | Yes (requires `--tsc`) |
| 200 response schema | `Responds<typeof Schema>` / `Promise<Responds<T>>` | Yes (requires `--tsc`) |
| Operation summary | First line of JSDoc on the method | Yes (requires `--tsc`) |
| Operation description | Full JSDoc body | Yes (requires `--tsc`) |
| Tags | Controller class name (default) or `@Tag` decorator | Partial |
| Non-200 response schemas | Not in method signature | No — needs `@Returns` |
| Security requirements | Not derivable from `@Middleware` name alone | No — needs `@Security` |
| Deprecated flag | Not in method signature | No — needs `@Deprecated` |

The zero-annotation surface covers all common CRUD routes. The annotation gap is narrow: non-200 responses, security schemes, and deprecated markers.

---

## Decision: `anvil-nestjs-openapi`

A standalone CLI tool, published as `@msulak/anvil-nestjs-openapi`, reads the same controller files as `anvil-nestjs-codegen` and emits an OpenAPI 3.1 document. It shares no runtime with `anvil-nestjs-codegen` and does not depend on `routes.module.ts` — it reads the original controller source directly.

```
anvil-nestjs-openapi --entry src/ --output openapi.yaml
```

### Schema conversion

The tool must convert runtime schema objects (Zod, Valibot, ArkType, etc.) to JSON Schema objects for the OpenAPI document. Two mechanisms are supported:

**Automatic Zod detection.** Zod schemas carry a `_def` property. The tool detects this structurally, imports `zod-to-json-schema` as an optional peer dependency, and converts automatically. No user configuration required for Zod projects.

**`JsonSchema` interface (other libraries).** The `Validator<T>` interface in `@msulak/anvil-plugin-nestjs` is extended with an optional method:

```typescript
export interface Validator<T> {
  safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown };
  jsonSchema?(): JSONSchema7;
}
```

Schema libraries that implement `jsonSchema()` are picked up automatically. For libraries that don't, users wrap the schema:

```typescript
import { withJsonSchema } from '@msulak/anvil-plugin-nestjs';
import * as v from 'valibot';

const GetByIdParamsSchema = withJsonSchema(
  v.object({ id: v.string() }),
  { type: 'object', properties: { id: { type: 'string' } }, required: ['id'] },
);
```

`withJsonSchema(validator, schema)` returns an object that satisfies both `Validator<T>` and `JsonSchema` — no change to the validation behaviour, just an attached JSON Schema descriptor.

### Deriving path parameters

`Params<typeof GetByIdParams>` tells the tool the extraction source is `req.params`. In `--tsc` mode the tool resolves `GetByIdParams` and converts its JSON Schema to a list of OpenAPI parameter objects with `in: "path"`. Property names in the schema become parameter names; `required` / optionality follows the schema's own required array. The parameter names must match the path template tokens (`:id` → `id`) — a mismatch is a diagnostic, not a silent gap.

Query parameters follow the same pattern with `in: "query"`. Body becomes a `requestBody` object with `content: { "application/json": { schema: ... } }`.

### Non-200 responses: `@Returns`

`Responds<T>` maps to the `200` response. Additional status codes are declared with a `@Returns` method decorator:

```typescript
@Post('/')
@Returns(201, typeof CreatedUserSchema)
@Returns(409, typeof ConflictSchema)
async create(body: Body<typeof CreateUserBody>): Promise<Responds<typeof CreatedUserSchema>>
```

`@Returns` arguments are non-literals (status code + schema reference) and require `--tsc`. The `Responds<T>` return type and `@Returns(201, ...)` may overlap — the tool uses `Responds<T>` for the primary success response and `@Returns` for all others. If `@Returns` declares a status that matches the `Responds<T>` schema (both 200 and `Responds<T>` reference the same schema), they are merged rather than duplicated.

Methods that declare `res: express.Response` and no `Responds<T>` emit a `responses: {}` object with a note that responses are handled manually, unless `@Returns` decorators are present.

### Security: `@Security`

`@Middleware(requireAuth)` cannot be automatically mapped to an OpenAPI security scheme — the identifier name is arbitrary user code, not a convention the tool can interpret. Security requirements are declared explicitly:

```typescript
@Controller('/admin')
@Security('bearerAuth')           // class-level: applies to all methods
@Inject
export class AdminController {
  @Delete('/:id')
  @Security('apiKey')             // method-level: adds or overrides
  async delete(params: Params<typeof DeleteParams>): Promise<Responds<typeof DeleteResponse>>
}
```

`@Security` arguments are string literals — the tool processes them in static mode. The scheme names must match entries in the document-level `securitySchemes` block, which is configured via the tool's config file rather than per-controller (scheme definitions are global, not per-route):

```yaml
# anvil-nestjs-openapi.config.yaml
info:
  title: My API
  version: 1.0.0
securitySchemes:
  bearerAuth:
    type: http
    scheme: bearer
  apiKey:
    type: apiKey
    in: header
    name: X-API-Key
```

### Deprecation: `@Deprecated`

```typescript
@Get('/:id')
@Deprecated('Use /v2/users/:id instead')
async byId(...): Promise<Responds<typeof GetByIdResponse>>
```

`@Deprecated` takes a string literal reason (shown in the description). The generated operation object gets `deprecated: true`. The reason is appended to the operation description.

### Tags

By default the tool derives the tag from the controller class name with the `Controller` suffix stripped (`UserController` → `users`, lowercased). `@Tag` overrides this:

```typescript
@Controller('/users')
@Tag('User Management')
@Inject
export class UserController { ... }
```

Tags are string literals; static mode handles them without `--tsc`.

### JSDoc descriptions

With `--tsc`, the tool reads JSDoc comments on handler methods via the TypeScript compiler API:

```typescript
/**
 * Retrieve a user by their unique identifier.
 *
 * Returns 404 if the user does not exist.
 */
@Get('/:id')
async byId(params: Params<typeof GetByIdParams>): Promise<Responds<typeof GetByIdResponse>>
```

The first sentence becomes `summary`; the full text becomes `description`. JSDoc is optional — absent comments leave `summary` and `description` undefined in the output, which is valid OpenAPI.

### Output format

The tool emits OpenAPI 3.1 YAML by default (more readable in diffs, universally supported). JSON output is available via `--format json`. The document passes validation against the OpenAPI 3.1 JSON Schema before being written; a validation failure is a tool error, not a silent bad document.

### Build integration

`anvil-nestjs-openapi` runs independently of `anvil-nestjs-codegen`. It does not need to run before or after `anvil build` — it reads source files directly and writes a static document. In CI the typical placement is after `tsc --noEmit` (so it runs on known-good source) and before publishing artifacts.

As a `postBuild` hook in `anvil-unplugin`:

```typescript
// vite.config.ts
import anvil from '@msulak/anvil-unplugin/vite';
import { nestjsCodegen } from '@msulak/anvil-plugin-nestjs/codegen';
import { nestjsOpenApi } from '@msulak/anvil-nestjs-openapi/unplugin';

export default {
  plugins: [
    anvil({
      entry: 'src/app.ts',
      preBuild:  [nestjsCodegen({ entry: 'src/' })],
      postBuild: [nestjsOpenApi({ entry: 'src/', output: 'openapi.yaml' })],
    }),
  ],
};
```

In watch mode the tool re-runs whenever a controller file changes, producing an updated `openapi.yaml` alongside the updated `routes.module.ts`.

---

## The annotation surface in full

A fully annotated controller using every available decorator:

```typescript
/**
 * Retrieve a user by their unique identifier.
 */
@Controller('/users')
@Tag('Users')
@Security('bearerAuth')
@Inject
export class UserController {
  constructor(private repo: UserRepository) {}

  @Get('/:id')
  @Returns(404, typeof NotFoundSchema)
  async byId(
    params: Params<typeof GetByIdParams>,
    query:  Query<typeof GetByIdQuery>,
  ): Promise<Responds<typeof GetByIdResponse>>

  @Post('/')
  @Returns(201, typeof CreatedUserSchema)
  @Returns(409, typeof ConflictSchema)
  @Security('apiKey')
  async create(
    body: Body<typeof CreateUserBody>,
  ): Promise<Responds<typeof CreatedUserSchema>>

  @Delete('/:id')
  @Deprecated('Users are soft-deleted as of v2; use PATCH /users/:id/deactivate')
  async delete(
    params: Params<typeof DeleteParams>,
    res:    express.Response,
  ): Promise<void>
}
```

The zero-annotation baseline (just `@Controller`, `@Get`, schema-typed params, `Responds<T>`) produces a valid, useful OpenAPI document for most routes. The additional decorators fill the narrow gap for non-200 responses, security, and deprecated operations.

---

## Consequences

**Good:**
- Zero-annotation coverage for the common case: path, method, all parameter schemas, and the 200 response are derived automatically from the method signature with no additional decorators.
- Controller methods remain focused on business logic. The OpenAPI metadata is either in the type signature (where it is enforced by the compiler) or in tightly scoped single-purpose decorators (`@Returns`, `@Security`, `@Deprecated`, `@Tag`).
- Schema conversion is library-agnostic: Zod is supported automatically; other libraries implement `jsonSchema()` or use `withJsonSchema()`.
- The spec is generated from source, not from the running application — it is always up to date with the code, not with whatever is deployed.
- No dependency on `routes.module.ts` — the OpenAPI tool can run before, after, or independently of `anvil-nestjs-codegen`.

**Bad:**
- Requires `--tsc` for schema resolution. Projects that avoid `--tsc` for build-time reasons get a spec with empty schemas unless all schemas implement `jsonSchema()` without compiler assistance.
- Non-200 responses require `@Returns` decorators. Methods that handle multiple status codes via `res.status(...)` calls but declare no `@Returns` produce incomplete specs. The tool emits a warning for methods with `res: express.Response` and no `@Returns`.
- Security scheme definitions live in a config file, not in code. A mismatch between a `@Security('bearerAuth')` annotation and the config file's `securitySchemes` is caught at spec-generation time (tool error), not at compile time.
- JSDoc descriptions are optional and therefore often absent. The spec is valid without them but less useful for consumers.

## Alternatives considered

- **Decorators for all fields (NestJS `@ApiProperty`, `@ApiOperation` style):** Fully explicit but verbose — every field that can be derived from the type signature must also be declared in a decorator. The derivation-first approach avoids redundancy; explicit decorators are reserved for the fields that genuinely cannot be derived.
- **Runtime spec generation (express-openapi, tsoa):** Generates the spec by inspecting the running application or by running a separate compilation pass over annotated classes. These tools typically require their own decorator set (`@Route`, `@Body`, `@Response`) and cannot reuse the `Body<T>`/`Responds<T>` types from `anvil-nestjs-codegen`. Using them alongside this system would mean maintaining two parallel annotation sets.
- **`openapi-typescript` / `typed-openapi` (schema-first):** Write the OpenAPI spec first and generate TypeScript types from it. This inverts the direction: types follow the spec rather than the spec following the types. Valid for greenfield API design but misaligned with the code-first approach of `anvil-nestjs-codegen`. Both directions can coexist in a monorepo (the generated spec could itself be fed into `openapi-typescript` for client generation).
- **Embedding JSON Schema in the `Validator<T>` interface as a required field:** Making `jsonSchema()` mandatory would force every schema reference in every controller to implement the interface, even in projects that don't use OpenAPI generation. Optional `jsonSchema()` keeps the runtime package lean for projects that only use `anvil-nestjs-codegen`.
