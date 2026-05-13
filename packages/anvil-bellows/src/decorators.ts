import type { RequestHandler } from "express";

export type ExpressMiddleware = RequestHandler;

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
  _schema: unknown,
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

export function Deprecated(
  _reason?: string,
): <This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
) => (this: This, ...args: Args) => Return {
  return (target, _ctx) => target;
}
