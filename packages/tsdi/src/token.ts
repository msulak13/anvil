/**
 * `Token<T>` lets users bind non-class types (interfaces, primitives, configs)
 * by associating them with a unique runtime identity.
 *
 * The codegen recognizes references to `Token<T>` instances in `@Provides`
 * return positions and `@Inject` parameter positions and uses them as the
 * binding's `Key`.
 *
 * Note: full `Token<T>` support lands in v0.2 (M7). The class is exported in
 * v0.1 only so user code that anticipates the migration typechecks today.
 *
 * @example
 * ```ts
 * import { Token } from "tsdi";
 * export interface Logger { log(msg: string): void; }
 * export const LOGGER = new Token<Logger>("LOGGER");
 * ```
 */
export class Token<T> {
  /** Phantom field that fixes the generic parameter and prevents structural collapse. */
  declare private readonly _brand: T;

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
