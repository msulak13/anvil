/**
 * No-op decorator stubs recognized by the `anvil` Rust codegen toolchain.
 *
 * These exist purely so user code typechecks (`import { Inject } from "@msulak/anvil"`).
 * The actual semantics are implemented at compile time by reading the decorator
 * AST and emitting wiring code into co-located `*.anvil.ts` files.
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
 * import { Module, Provides } from "@msulak/anvil";
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
 * import { Inject } from "@msulak/anvil";
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
 * Marks a static method on a `@Module` class as an alias-binding from
 * the method's return type to its single parameter type. The codegen
 * emits a factory that delegates straight to the parameter type's
 * factory — useful for binding an interface (or abstract base class) to
 * one concrete impl.
 *
 * The method must be `static`, take exactly one parameter, and declare
 * an explicit return type. It must not also be `@Provides`. The body
 * is required so the file type-checks, but the codegen ignores it and
 * emits a delegate to the parameter's factory.
 *
 * (TC39 Stage-3 decorators cannot decorate `abstract` methods —
 * TS error 1249 — which is why `@Binds` uses a static method with a
 * trivial `return impl;` body rather than the Dagger-style abstract form.)
 *
 * @example
 * ```ts
 * import { Module, Binds } from "@msulak/anvil";
 * import { Heater } from "./heater";
 * import { ElectricHeater } from "./electric-heater";
 *
 * @Module
 * export class CoffeeModule {
 *   @Binds static heater(impl: ElectricHeater): Heater { return impl; }
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
 * emits a sibling `*.anvil.ts` file containing a concrete subclass that
 * satisfies every abstract method using the configured modules.
 *
 * @example
 * ```ts
 * import { Component } from "@msulak/anvil";
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
 * Configuration for a `@Subcomponent`. Mirrors {@link ComponentConfig}.
 */
export interface SubcomponentConfig {
  /** Module classes whose `@Provides` methods are visible to this subcomponent. */
  readonly modules?: ReadonlyArray<abstract new (...args: never[]) => unknown>;
}

/**
 * Marks an abstract class as a child object graph nested inside a parent
 * `@Component` (or another `@Subcomponent`). A subcomponent inherits all
 * bindings declared by its parent and adds its own modules on top.
 *
 * The parent exposes the child by declaring an abstract zero-arg method
 * whose return type is the subcomponent class; the codegen emits a
 * factory that constructs `Dagger<Sub>` with the parent dagger as a
 * back-reference, so inherited dependencies route through the parent's
 * factories (and shared singletons stay shared).
 *
 * @example
 * ```ts
 * import { Component, Subcomponent } from "@msulak/anvil";
 * import { RequestModule } from "./request-module";
 *
 * @Subcomponent({ modules: [RequestModule] })
 * export abstract class RequestComponent {
 *   abstract handler(): RequestHandler;
 * }
 *
 * @Component({ modules: [AppModule] })
 * export abstract class AppComponent {
 *   abstract requestComponent(): RequestComponent;
 * }
 * ```
 */
export function Subcomponent(_config: SubcomponentConfig = {}) {
  return <T extends abstract new (...args: never[]) => unknown>(
    target: T,
    _ctx: ClassDecoratorContext<T>,
  ): T => target;
}

/**
 * Declares that instances of the decorated class are cached for the lifetime
 * of the owning component (i.e. one-per-component).
 */
// Method-decorator overload: `@Singleton @Provides static fn(...): T`.
export function Singleton<This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
): (this: This, ...args: Args) => Return;
// Class-decorator overload: `@Inject @Singleton class X {}`.
export function Singleton<T extends abstract new (...args: never[]) => unknown>(
  target: T,
  ctx: ClassDecoratorContext<T>,
): T;
// Implementation — both decorator shapes are no-ops at runtime; anvil's
// codegen reads the @Singleton presence from the AST and emits cache
// fields accordingly.
export function Singleton(target: unknown, _ctx: unknown): unknown {
  return target;
}

/**
 * Marks an `@Provides` method as a contribution to a `Set<T>` multibinding,
 * where `T` is the method's return type. The codegen aggregates every
 * contribution to the same element type into a single factory that returns
 * a `Set<T>` populated with each contributor's result.
 *
 * Multiple `@IntoSet` contributions to the same element type are not
 * duplicates — they are intentionally collected. A consumer requests the
 * aggregate by typing a parameter or entry-point return as `Set<T>`.
 *
 * Must be combined with `@Provides` (not `@Binds` or `@Inject`) for v0.1.
 *
 * @example
 * ```ts
 * import { Module, Provides, IntoSet, Component } from "@msulak/anvil";
 *
 * @Module
 * export class PluginsModule {
 *   @Provides @IntoSet
 *   static auth(): Plugin { return new AuthPlugin(); }
 *
 *   @Provides @IntoSet
 *   static logging(): Plugin { return new LoggingPlugin(); }
 * }
 *
 * @Component({ modules: [PluginsModule] })
 * export abstract class App {
 *   abstract plugins(): Set<Plugin>;
 * }
 * ```
 */
export function IntoSet<This, Args extends readonly unknown[], Return>(
  target: (this: This, ...args: Args) => Return,
  _ctx: ClassMethodDecoratorContext<This, (this: This, ...args: Args) => Return>,
): (this: This, ...args: Args) => Return {
  return target;
}
