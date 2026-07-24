import type { Request } from "express";

export interface AuthnResult<U = unknown> {
  identified: boolean;
  user?: U;
}

/**
 * `Scheme` is never evaluated at runtime — it's read directly off the
 * `implements AuthnService<User, "bearerAuth">` clause by anvil-bellows'
 * codegen to populate the OpenAPI `security` requirement for routes that
 * declare this service via `@Authn(...)`.
 */
export interface AuthnService<U = unknown, Scheme extends string = never> {
  identify(req: Request): AuthnResult<U> | Promise<AuthnResult<U>>;
}

export type AuthzDecision = "allow" | "deny" | "next";

export interface AuthzService {
  authorize(req: Request, user: unknown): AuthzDecision | Promise<AuthzDecision>;
}

/**
 * Marks a handler parameter for injection of the identified user
 * (`res.locals.user`, set by the route's `@Authn` cascade). `T` is a pure
 * type-level marker — anvil-bellows' codegen statically verifies `T`
 * identity-matches the `U` declared by every `@Authn` service on the route
 * (`implements AuthnService<U, Scheme>`), rejecting the route at build time
 * if they disagree or can't be resolved. At runtime this is just `T`.
 */
export type AuthnUser<T> = T;
