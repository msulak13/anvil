/**
 * `Token<T, Name>` is the type-annotation form for named binding keys.
 *
 * The codegen (M14+) recognizes `Token<T, "name">` as a type annotation in
 * `@Provides` return types and `@Inject` constructor parameters and uses the
 * string literal `"name"` as the binding's `Key::Token`. No runtime instance
 * is needed — the name is read purely from the AST.
 *
 * The optional second type parameter `Name` carries the string-literal key
 * so that two parameters annotated `Token<Database, "primary">` and
 * `Token<Database, "replica">` are structurally distinct types that TypeScript
 * will not treat as interchangeable.
 *
 * @example
 * ```ts
 * // db-module.ts
 * import { Module, Provides } from "@msulak/anvil";
 * import type { Token } from "@msulak/anvil";
 * import type { Database } from "./database";
 *
 * @Module
 * export class DbModule {
 *   @Provides
 *   static primaryDb(): Token<Database, "primary-db"> {
 *     return new Database(primaryUrl) as unknown as Token<Database, "primary-db">;
 *   }
 * }
 *
 * // app-component.ts
 * import { Component, Inject } from "@msulak/anvil";
 * import type { Token } from "@msulak/anvil";
 * import type { Database } from "./database";
 *
 * @Inject
 * export class UserRepository {
 *   constructor(private db: Token<Database, "primary-db">) {}
 * }
 * ```
 */
// eslint-disable-next-line @typescript-eslint/no-unused-vars
export class Token<T, Name extends string = string> {
  /** Phantom fields that fix the generic parameters and prevent structural collapse. */
  declare private readonly _brand: T;
  declare private readonly _name: Name;

  /**
   * @param description Human-readable label, used in diagnostic output. Not
   *   required to be unique — the runtime identity is the `Token` instance.
   */
  constructor(public readonly description: string) {}

  /** @returns the description, prefixed for easy log identification. */
  toString(): string {
    return `Token<${this.description}>`;
  }
}
