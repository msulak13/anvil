# 0008 — Composite Decorators for Bellows Controllers

**Status:** Proposed
**Date:** 2026-11-27
**Depends on:** ADR-0003 (no type checker), ADR-0006 (`anvil-bellows`)

## Context

Controllers repeat the same decorator stacks on every class or route that shares a policy:

```typescript
@Controller("/admin/users")
@Authn(SessionAuthn)
@Authz(AdminAuthz)
@Middleware(auditLog, rateLimit)
@Tag("admin")
export class AdminUsersController {}
```

Decorators are no-ops at runtime; `anvil-bellows` reads them statically by bare callee name. A user-written wrapper function is invisible to the parser, so the stack cannot be factored out today. A forgotten or reordered `@Authn` on one controller is a security bug that nothing flags.

## Decision

### API

`@anvil-di/bellows` exports `bellowsCompose`, a no-op stub whose call shape is the contract the parser reads:

```typescript
export type BellowsDecorator = (
  target: unknown,
  ctx: ClassDecoratorContext | ClassMethodDecoratorContext,
) => void;

export function bellowsCompose(
  ..._decorators: ((...args: never[]) => unknown)[]
): BellowsDecorator {
  return () => {};
}
```

```typescript
// src/policies.ts
export const Authenticated = bellowsCompose(
  Authn(SessionAuthn),
  Returns(401),
);

export const AdminRoute = bellowsCompose(
  Authenticated,
  Authz(AdminAuthz),
  Middleware(auditLog, rateLimit),
  Tag("admin"),
);

// src/admin-users.controller.ts
@Controller("/admin/users")
@AdminRoute
export class AdminUsersController {}
```

`BellowsDecorator` has one non-overloaded signature, so the same composite type-checks on classes, instance methods, and static methods, and the call form `@AdminRoute()` is a `tsc` error.

### Recognized definition shape

A composite is a top-level `const <Ident> = bellowsCompose(...)`, exported or not. Each argument is either:

- a call to `Middleware`, `Authn`, `Authz`, `Tag`, `Security`, or `Returns(<status>)`, with the same literal/identifier rules as at a use site; or
- a bare identifier naming another composite.

Any other argument is a diagnostic at the definition.

### Resolution

A bare-identifier decorator (`@X`, not `@X(...)`) on a `@Controller` class or route method resolves by `X`'s origin in the use-site file's import map:

| Origin | Behavior |
|---|---|
| None (declared in the use-site file) | Find the `const` in that file. |
| `ImportOrigin::Relative(path)` | Parse `path` and find the `const`, the same way `resolve_authn_type_args` reads `AuthnService` classes. |
| `ImportOrigin::Package` | Not expanded. |

A bare decorator that doesn't resolve to a `bellowsCompose` `const` is an ordinary decorator and is left alone. Resolved definitions are cached per `(file, name)`. Nested composites resolve recursively against the definition file's import map; a cycle is a diagnostic.

### Expansion

- A composite expands in place, depth-first, in argument order, into the per-kind lists the parser already builds (`middleware`, `authn`, `authz`, `tags`, `security`, `extra_responses`).
- A class-level composite contributes to the class-level lists; a method-level one to the method-level lists. The existing rule (class-level entries first, then method-level, each in source order) is unchanged.
- Identifiers inside a composite resolve against the **definition** file's import map. A ref declared in the definition file (`origin: None` there) is rewritten to `Relative(definition_file)`, because `None` means "declared in the controller file" to codegen.
- Composites produce no new IR. `Route` and `Controller` carry flattened lists, so `routes.module.ts` and the OpenAPI document are identical to the hand-written stack.

### Placement

A composite may only contain kinds the parser reads at its use site:

| Kind | Class | Method |
|---|---|---|
| `Middleware`, `Authn`, `Authz` | ✓ | ✓ |
| `Tag`, `Security` | ✓ | ✗ |
| `Returns(<status>)` | ✗ | ✓ |

An unsupported kind at the use site is a diagnostic, not a silent drop.

### Diagnostics

Each of these is a `ParseDiagnostic` (exit code 1):

- A composite contains `@Controller`, a route decorator, `@Deprecated`, or `@Returns` with a schema argument.
- A composite contains a kind that isn't allowed where it's used.
- Composites reference each other in a cycle.

## Consequences

- A policy is defined once instead of copied onto every controller.
- No IR, codegen, or OpenAPI builder changes. The work is in the `anvil-bellows` parser plus the runtime stub.
- The per-kind `try_extract_*` functions collapse into one classifier that returns an owned `DecoratorItem` enum. Direct decorators and composite contents share it, which is needed because a composite's AST lives in a different allocator than the controller's.
- `anvil-bellows-openapi` uses the same parser, so its tags, security, and responses pick up composites automatically.
- A composite imported from a package or through a barrel re-export isn't expanded, and the build doesn't flag it, so its policies don't apply. Supporting these needs `anvil-bellows` to resolve bare specifiers and follow re-exports.
- `@Returns(status, Schema)` isn't allowed inside composites, because `SchemaRef` has no origin to carry the definition file's import.
- Watch mode reruns only for files under `entry` (`bellowsCodegen().shouldRerun`). A composite defined outside `entry` doesn't trigger a rebuild when it changes.

## Alternatives considered

- **Read arbitrary user functions** (`function AdminRoute(t, c) { Authn(X)(t, c); … }`). The parser would need to evaluate code. One fixed call shape keeps recognition purely syntactic (ADR-0003).
- **Array spread** (`@Middleware(...adminMiddleware)`). Covers one decorator kind per array, not a whole policy.
- **Class inheritance** (`class AdminUsersController extends AdminBase`). Class-level only; method-level policies can't use it.
- **Require every bare decorator to resolve to a composite.** This would catch composites that fail to resolve, but it would break ordinary decorators on controllers.
- **Call-form composites** (`@AdminRoute()`). They make room for parameters without a design for them; bare form keeps a composite a value.
