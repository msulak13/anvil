/**
 * No-op decorator stubs recognized by the `tsdi` Rust codegen toolchain.
 *
 * These exist purely so user code typechecks (`import { Inject } from "tsdi"`).
 * The actual semantics are implemented at compile time by reading the decorator
 * AST and emitting wiring code into co-located `*.tsdi.ts` files.
 *
 * All decorators are TC39 Stage-3 ([decision recorded in](../../../docs/adr/0002-stage3-decorators-only.md))
 * and do not require `experimentalDecorators` in `tsconfig.json`.
 */

/**
 * Marks a class as a `@Module` whose static methods (annotated with `@Provides`)
 * contribute bindings to a component's graph.
 *
 * @example
 * ```ts
 * import { Module, Provides } from "tsdi";
 * import { Pump } from "./pump";
 *
 * @Module
 * export class CoffeeModule {
 *   @Provides static providePump(p: Pump): Pump { return p; }
 * }
 * ```
 */
export function Module<T extends abstract new (...args: never[]) => unknown>(
  target: T,
  _ctx: ClassDecoratorContext<T>,
): T {
  return target;
}

/**
 * Marks a static method on a `@Module` class as a binding factory.
 *
 * The method's return type is the bound key; its parameters are the dependencies.
 * Must be `static` and have an explicit return type — both rules are enforced
 * at codegen time.
 */
export function Provides<This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  _ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
): (this: This, ...args: Args) => Return {
  return target;
}

/**
 * Marks a class for injection by the codegen toolchain. The class's
 * constructor parameter types become the binding's dependencies.
 *
 * `@Inject` is a class-level decorator — TC39 Stage-3 decorators do not
 * apply to constructors, so the legacy `@Inject constructor(...)` placement
 * is rejected by the toolchain.
 *
 * @example
 * ```ts
 * import { Inject } from "tsdi";
 * import { Heater } from "./heater";
 *
 * @Inject
 * export class Pump {
 *   constructor(private heater: Heater) {}
 * }
 * ```
 */
export function Inject<T extends abstract new (...args: never[]) => unknown>(
  target: T,
  _ctx: ClassDecoratorContext<T>,
): T {
  return target;
}

/**
 * Marks an abstract method on a `@Module` class as an alias-binding from
 * the method's return type to its single parameter type. The codegen
 * emits a factory that delegates straight to the parameter type's
 * factory — useful for binding an interface to one concrete impl.
 *
 * The method must be `abstract`, take exactly one parameter, and declare
 * an explicit return type. It must not also be `@Provides`.
 *
 * @example
 * ```ts
 * import { Module, Binds } from "tsdi";
 * import { Heater } from "./heater";
 * import { ElectricHeater } from "./electric-heater";
 *
 * @Module
 * export abstract class CoffeeModule {
 *   @Binds abstract heater(impl: ElectricHeater): Heater;
 * }
 * ```
 */
export function Binds<This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  _ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
): (this: This, ...args: Args) => Return {
  return target;
}

/**
 * Configuration for a `@Component`.
 */
export interface ComponentConfig {
  /** Module classes whose `@Provides` methods are visible to this component. */
  readonly modules?: ReadonlyArray<abstract new (...args: never[]) => unknown>;
}

/**
 * Marks an abstract class as the root of an object graph. The Rust codegen
 * emits a sibling `*.tsdi.ts` file containing a concrete subclass that
 * satisfies every abstract method using the configured modules.
 *
 * @example
 * ```ts
 * import { Component } from "tsdi";
 * import { CoffeeModule } from "./coffee-module";
 *
 * @Component({ modules: [CoffeeModule] })
 * export abstract class CoffeeShop {
 *   abstract pump(): Pump;
 * }
 * ```
 */
export function Component(_config: ComponentConfig = {}) {
  return <T extends abstract new (...args: never[]) => unknown>(
    target: T,
    _ctx: ClassDecoratorContext<T>,
  ): T => target;
}

/**
 * Declares that instances of the decorated class are cached for the lifetime
 * of the owning component (i.e. one-per-component).
 */
export function Singleton<T extends abstract new (...args: never[]) => unknown>(
  target: T,
  _ctx: ClassDecoratorContext<T>,
): T {
  return target;
}
