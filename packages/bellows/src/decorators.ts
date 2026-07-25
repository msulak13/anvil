import type { RequestHandler } from "express";
import type { AuthnService, AuthzService } from "./authz.js";

export type ExpressMiddleware = RequestHandler;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AuthnServiceClass = new (...args: never[]) => AuthnService<any, any>;
type AuthzServiceClass = new (...args: never[]) => AuthzService;

export function Controller(
  _path: string,
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T {
  return (target, _ctx) => target;
}

export function Get(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

export function Post(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

export function Put(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

export function Delete(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

export function Patch(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

/**
 * Marks a route as a Server-Sent Events stream (`text/event-stream`) rather
 * than a buffered JSON response. Registered as a `GET` route. The handler
 * must return `void`/`Promise<void>` and manages the connection itself via
 * an injected `SseStream` param — see `@anvil-di/bellows`'s `SseStream`.
 */
export function Sse(
  _path: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

/**
 * Usable as either a class or method decorator: `@Middleware(fn) class Ctrl {}`
 * or `@Middleware(fn) handler() {}`. Deliberately has no typed overloads —
 * TS's decorator-overload resolution always binds the first matching
 * signature regardless of the actual target, so a class-decorator overload
 * and a method-decorator overload with identical outer parameter lists can't
 * coexist (whichever is declared second fails to typecheck at its call
 * sites). A single loosely-typed signature checks correctly at both.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Middleware(..._fns: ExpressMiddleware[]): any {
  return (target: unknown) => target;
}

export function Tag(
  _name: string,
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T {
  return (target, _ctx) => target;
}

export function Returns(
  _status: number,
  _schema?: unknown,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}

/**
 * Usable as either a class or method decorator — see the note on
 * `Middleware`'s signature above for why this has no typed overloads.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Security(_scheme: string): any {
  return (target: unknown) => target;
}

/**
 * Usable as either a class or method decorator — see the note on
 * `Middleware`'s signature above for why this has no typed overloads.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Authn(..._services: AuthnServiceClass[]): any {
  return (target: unknown) => target;
}

/**
 * Usable as either a class or method decorator — see the note on
 * `Middleware`'s signature above for why this has no typed overloads.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Authz(..._services: AuthzServiceClass[]): any {
  return (target: unknown) => target;
}

export function Deprecated(
  _reason?: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}
