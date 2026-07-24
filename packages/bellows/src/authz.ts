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
