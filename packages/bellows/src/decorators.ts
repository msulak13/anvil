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

// Class-decorator overload: `@Middleware(fn) class Ctrl {}`
export function Middleware(
  ...fns: ExpressMiddleware[]
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T;
// Method-decorator overload: `@Middleware(fn) handler() {}`
export function Middleware(
  ...fns: ExpressMiddleware[]
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return;
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

// Class-decorator overload: `@Security("bearer") class Ctrl {}`
export function Security(
  scheme: string,
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T;
// Method-decorator overload: `@Security("bearer") handler() {}`
export function Security(
  scheme: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Security(_scheme: string): any {
  return (target: unknown) => target;
}

// Class-decorator overload: `@Authn(SessionAuthn) class Ctrl {}`
export function Authn(
  ...services: AuthnServiceClass[]
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T;
// Method-decorator overload: `@Authn(SessionAuthn) handler() {}`
export function Authn(
  ...services: AuthnServiceClass[]
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function Authn(..._services: AuthnServiceClass[]): any {
  return (target: unknown) => target;
}

// Class-decorator overload: `@Authz(RoleAuthz) class Ctrl {}`
export function Authz(
  ...services: AuthzServiceClass[]
): <T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
) => T;
// Method-decorator overload: `@Authz(RoleAuthz) handler() {}`
export function Authz(
  ...services: AuthzServiceClass[]
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return;
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
