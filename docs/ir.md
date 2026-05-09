# IR specification

The internal representation of the binding graph. This is the **stable contract** between `anvil-parser` and `anvil-codegen`. Definitions live in [`crates/anvil-core/src/ir.rs`](../crates/anvil-core/src/ir.rs).

Update this page whenever a variant is added or removed.

## `Key`

Stable identity for a TypeScript type without a full type checker. Two values are equal iff they refer to the same exported declaration in the same source file.

```rust
pub enum Key {
    Class { module: ModulePath, name: String },
    Set   { element: Box<Key> },                  // M9 — Set<T> multibinding aggregate
    // Token { module: ModulePath, name: String },  // v0.2
}

pub struct ModulePath {
    pub abs: String,                // M1: raw import specifier; M2+: absolute, normalized path
    pub original: Option<String>,   // M10: user's original specifier (e.g. "express", "./pump")
}
```

`abs` carries identity — `ModulePath` equality and hashing use it alone, so two importers spelling the same file differently still produce equal `Key`s. `original` carries provenance: codegen prefers it for `node_modules` imports so the dagger emits `import { Request } from "express"` instead of an `import { Request } from "../../node_modules/..."` relative path that would only resolve from one specific output location.

The parser populates `original = Some(spec)` whenever a `Key` is minted from an import-map entry. The M2 resolver rewrites `abs` to the canonical filesystem path while leaving `original` alone. For same-file references and tooling-built paths (graph tests, golden fixtures), `original` is `None` and codegen falls back to a relative-path computation.

`Key::Set` represents a `Set<T>` aggregate produced from one or more `@IntoSet @Provides` contributions. The `element` is always a `Key::Class` in v0.1 (no `Set<Set<T>>`-of-`Set` chains in user code). The graph aggregator synthesizes one `Provider::SetMultibinding` binding per element key from the raw `MultibindRole::IntoSet` bindings the parser emits (see [Multibindings](#multibindings) below).

In **M1** the parser stores the *raw import specifier* in `ModulePath` (e.g. `"./heater"`, `"@msulak/anvil"`, `"my-pkg/sub"`). M2's cross-file resolver rewrites these to absolute paths so equivalent imports compare equal. For type identifiers declared in the same file as their reference, the parser uses the sentinel `ModulePath::SAME_FILE` (`"<self>"`); M2 swaps it for the file's actual absolute path.

### How a `Key` is minted

For a constructor parameter `private heater: Heater`:

1. Parser sees the type annotation identifier `Heater`.
2. Looks it up in the file's import map: `import { Heater } from "./heater"`.
3. Resolves `"./heater"` relative to the current file → absolute path.
4. Yields `Key::Class { module: <abs path>, name: "Heater" }`.

For default exports, the import binding name is used (TS makes default exports name-agnostic; we treat `import Heater from ...` and `import { Heater } from ...` distinctly because their resolution is different).

### Why no full type checker

Running `tsc` from Rust is too slow for watch mode, and `stc` (the abandoned Rust TS type checker) is not viable. Module-path identity is sufficient for class-typed bindings — the only shape supported in v0.1. Non-class types (interfaces, primitives, configs) require a `Token<T>` indirection (M7+).

See [`adr/0003-no-type-checker.md`](./adr/0003-no-type-checker.md).

## `Scope`

```rust
pub enum Scope { Unscoped, Singleton }
```

`Singleton` means one instance per owning component. `Unscoped` means a fresh instance per request.

The mismatch rule (M6): an unscoped binding **may** depend on a singleton (the singleton's identity is preserved); a singleton **may not** depend on an unscoped one (would invalidate the lifetime guarantee). Custom scopes (`Scope::Custom(String)`) arrive in v0.2.

## `ClassRef`

A reference to a class declaration in source.

```rust
pub struct ClassRef {
    pub module: ModulePath,
    pub name: String,
}
```

Used wherever the IR needs to point at a class without representing the binding semantics — e.g. the host of a `@Module`, the target of `@Inject` constructor injection, the abstract class on a `@Component`.

## `Provider`

How an instance for a `Key` is produced.

```rust
pub enum Provider {
    InjectCtor      { class: ClassRef },
    ProvidesMethod  { module: ClassRef, method: String, is_async: bool },
    Binds           { target: Key },
    SetMultibinding { contributors: Vec<SetContributor> },  // M9 — synthesized
    FactoryParam   { name: String },                         // M11 — synthesized
}

pub struct SetContributor {
    pub module: ClassRef,
    pub method: String,
    pub deps: Vec<Key>,
    pub source: SourceSpan,
}
```

- **`InjectCtor`** — class with class-level `@Inject` decorator. The constructor's parameter types become the binding's `deps`. The codegen emits `new ClassName(deps...)`.
- **`ProvidesMethod`** — static method on a `@Module`. The codegen emits `ModuleName.methodName(deps...)`. M12: `is_async` is set when the method is declared `async` (the parser unwraps the `Promise<T>` return-type annotation into the inner key). In an async graph, async `ProvidesMethod` factories are awaited inside the dagger's `static async _resolve()` phase and the **resolved** value is cached. See [Async `@Provides`](#async-provides-m12) below.
- **`Binds`** (M7) — alias binding. The owning `@Module` exposes a `static` method whose single parameter type is the implementation and whose return type is the alias. The binding's `key` is the return type (the alias); `target` is the parameter type (the implementation). `deps` is `vec![target]` so the topo walk visits the target's binding before the alias's factory references it. Codegen emits `return this.<getTarget>()` — no `new` is performed by the alias factory; the target's scope governs caching.

  TC39 Stage-3 decorators cannot decorate abstract methods (TS error 1249), so `@Binds` methods are `static` with a body. `anvil-codegen` ignores the body and emits the delegate; the body still has to compile (e.g. `return impl;`) so the user's `tsc` accepts the source file.

- **`FactoryParam`** (M11) — synthesized virtual binding for a runtime value supplied to a `@Subcomponent` factory's parameter list. The graph layer injects one `Provider::FactoryParam { name }` binding per parameter into the child's binding map; codegen materializes that as a `private <name>: T` field on the child dagger plus a trivial `private get<T>(): T { return this.<name>; }` getter so dep-call sites stay uniform. See [Subcomponent factory parameters](#subcomponent-factory-parameters-m11) below.

- **`SetMultibinding`** (M9) — synthesized aggregate. Never produced by the parser directly. The graph aggregator walks every `@IntoSet @Provides` raw binding (carrying `role: MultibindRole::IntoSet`), groups them by element type, and emits one `Provider::SetMultibinding` under `Key::Set { element }`. Each `SetContributor` records the originating `@Module` class, method name, and that contributor's own deps. Codegen emits `return new Set([Mod1.foo(...), Mod2.bar(...)])` — one call per contributor with its own dep argument list.

## `SourceSpan`

```rust
pub struct SourceSpan {
    pub path: String,   // absolute, canonical source-file path (M2+)
    pub start: u32,     // inclusive byte offset
    pub end: u32,       // exclusive byte offset
}
```

A parser-agnostic byte range used by validation diagnostics. Kept free of any `oxc_*` types so `anvil-core` stays parser-independent. The parser converts each `oxc_span::Span` into a `SourceSpan` at extraction time (M1+); the M2 resolver rewrites every `path` field to absolute form alongside the same canonicalization it does for `ModulePath`.

## `Binding`

```rust
pub struct Binding {
    pub key: Key,
    pub provider: Provider,
    pub scope: Scope,
    pub deps: Vec<Key>,
    pub source: SourceSpan,        // M3+: where the binding appears in source
    pub role: MultibindRole,       // M9 — multibinding contribution role
}

pub enum MultibindRole {
    None,        // regular binding
    IntoSet,     // a contribution to a Set<T> aggregate
}
```

A single contribution to the graph. The `deps` are the keys the provider needs to construct its output. The `source` field anchors validation diagnostics on the right line.

`role` is set by the parser when it sees `@IntoSet` on a `@Provides` method. The graph aggregator consumes raw bindings with `role != None`, groups them, and emits a synthesized `Provider::SetMultibinding` binding (whose own `role` is `None`). Downstream consumers — codegen, `explain`, validation — only ever see `MultibindRole::None` because the aggregation pass strips raw contributions out of the binding map.

## `ModuleDecl`

```rust
pub struct ModuleDecl {
    pub class: ClassRef,
    pub provides: Vec<Binding>,
    pub source: SourceSpan,
}
```

A class annotated `@Module`. Each `@Provides` static method becomes a `Binding` in `provides`.

## `ComponentDecl`

```rust
pub struct ComponentDecl {
    pub class: ClassRef,
    pub modules: Vec<ClassRef>,
    pub scope: Scope,
    pub entry_points: Vec<EntryPoint>,
    pub source: SourceSpan,
}

pub struct EntryPoint {
    pub name: String,   // method name on the abstract class
    pub key: Key,       // method's return type
    pub source: SourceSpan,
}
```

The root of an object graph. Each abstract method on the user's `@Component` class becomes an `EntryPoint`.

## `SubcomponentDecl`

```rust
pub struct SubcomponentDecl {
    pub class: ClassRef,
    pub modules: Vec<ClassRef>,
    pub scope: Scope,
    pub entry_points: Vec<EntryPoint>,
    pub source: SourceSpan,
}
```

A class annotated `@Subcomponent` (M8). Structurally identical to `ComponentDecl`; the distinction is *who builds it*. A subcomponent does not stand alone — it is reached through a parent component's abstract zero-arg method whose return type names the subcomponent class. The graph-layer wires this up: when an entry point's `key` matches a known `SubcomponentDecl`, the graph builder produces a `SubcomponentFactory` that owns a child `DependencyGraph` resolved with the parent's bindings as a fallback. Keys satisfied by the parent are recorded in `child_graph.inherited_keys` so codegen can route them through `this.parent.<getX>()` instead of constructing a fresh instance.

Subcomponents inherit the parent's scope cache for inherited bindings — a `@Singleton` `Heater` provided by the parent stays one instance across every child request.

## Subcomponent factory parameters (M11)

A `@Subcomponent`'s parent factory method may declare formal parameters. Each parameter becomes a virtual binding inside the child graph that resolves to the runtime value supplied at the factory call site:

```ts
@Component({ modules: [] })
export abstract class App {
  abstract requestComponent(req: HttpRequest, res: HttpResponse): RequestComponent;
}
```

Pipeline:

1. Parser captures `factory_params: [{name: "req", key: HttpRequest, …}, {name: "res", key: HttpResponse, …}]` on the `requestComponent` `EntryPoint`. Regular `@Component` entry points still have `factory_params: vec![]`.
2. M2's resolver normalizes each `FactoryParam.key.module.abs` to an absolute path the same way it does for every other `Key`.
3. Graph layer (`build_child_graph`) sees the parent factory's `factory_params` and **injects** a `Binding { provider: Provider::FactoryParam { name }, scope: Unscoped, deps: [], … }` per parameter into the child's binding map *before* `build_petgraph` runs. Any child binding requesting `HttpRequest` resolves locally — never as inherited from the parent.
4. Three new validation rules guard misuse: `FactoryParamsOnNonSubcomponentEntry` (params on a regular `@Component` method), `DuplicateFactoryParam` (two params with the same Key), `SingletonSubcomponentWithFactoryParams` (a singleton subcomponent that takes runtime args would freeze them across calls).
5. Codegen emits the parent factory as `requestComponent(req: HttpRequest, res: HttpResponse): RequestComponent { return new DaggerRequestComponent(this, req, res); }` and the child class as `constructor(private parent, private req: HttpRequest, private res: HttpResponse) { super(); }` plus a private `getHttpRequest(): HttpRequest { return this.req; }` getter per parameter so dep-call sites stay uniform.

Reachability pruning was added alongside M11: the graph builder now does a BFS from non-subcomponent entry points and drops any binding (including project-wide `@Inject` self-bindings) not transitively reached. Without this, a subcomponent-only `@Inject` class would leak onto the parent dagger as a non-private factory and trigger spurious missing-dep diagnostics for its child-only deps.

## Async `@Provides` (M12)

A `@Provides` method declared `async` returns `Promise<T>`. anvil unwraps the `Promise<T>` for the binding key (so consumers see the resolved type, not `Promise<T>`) and sets `is_async: true` on the provider. The dagger's resolution semantics:

```ts
@Module
class DatabaseModule {
  @Singleton @Provides static async pool(c: Config): Promise<Pool> {
    return await createPool(c);
  }
}

@Singleton @Component({ modules: [ConfigModule, DatabaseModule] })
abstract class App {
  abstract pool(): Pool;        // sync — pool already resolved
}

// Usage
const app = await createApp();  // ← awaits all async @Provides at startup
app.pool().query("…");           // sync from here on
```

Pipeline:

1. Parser detects `async` on the method and unwraps `Promise<T>` to the inner `Key`. Provider becomes `ProvidesMethod { is_async: true, … }`.
2. M2 resolver normalizes module paths same as any other binding — async-ness is orthogonal to identity.
3. Graph layer exposes `DependencyGraph::is_async()` (any reachable binding async?) and `binding_is_async(&Key)` (this specific binding async?). New validation rule `AsyncBindingNeedsSingletonComponent` rejects async `@Provides` outside `@Singleton` components — there'd be no place to cache the awaited value, every entry-point call would re-await, and the entry-point method itself would have to become async (viral). Subcomponents are exempt because they're already fresh per-call.
4. Codegen emits a `static async _resolve(d: DaggerX): Promise<void>` method that walks the topo array assigning `d._x = await Module.method(d.getDep())` for async `@Provides` and `d._x = new Class(d.getDep())` for sync Singleton bindings in the same graph. Sync `getX()` getters return `this._x!`. `static create()` becomes `async` and `createX()` returns `Promise<X>`. Subcomponent factories on the parent become `async requestComponent(req, res): Promise<RequestComponent>` when the child graph is async.
5. `@Inject` constructors stay synchronous — TS forbids `async constructor`, and any async work must live in `@Provides`. The `ExtractError::AsyncInjectCtor` variant exists as defense-in-depth; Oxc rejects the syntax before anvil-parser sees it.

Out of scope for v0.2: lazy async resolution (every call awaits on demand), `Promise<T>` as a directly-injectable binding type (would require `Token<Promise<T>>`), async `@Inject` factories.

## Multibindings

M9 adds `@IntoSet`. Multiple `@IntoSet @Provides` methods on the same `@Module` (or across modules a component includes) collectively produce a `Set<T>`:

```ts
@Module
export class PluginsModule {
  @IntoSet @Provides static auth(): Plugin    { return new AuthPlugin(); }
  @IntoSet @Provides static logging(): Plugin { return new LoggingPlugin(); }
}

@Component({ modules: [PluginsModule] })
export abstract class App {
  abstract plugins(): Set<Plugin>;
}
```

Pipeline:

1. Parser emits two raw `Binding`s with `key = Key::Class { ... "Plugin" }`, `provider = ProvidesMethod`, and `role = MultibindRole::IntoSet`.
2. Graph aggregator (`anvil-core::graph::aggregate_bindings`) folds them: it groups all `IntoSet` raw bindings by `key`, lifts the key to `Key::Set { element: Box::new(plugin_key) }`, and constructs a synthesized `Binding` whose provider is `Provider::SetMultibinding { contributors: [...] }`. The synthesized binding's `deps` is the union of every contributor's deps. Multiple `@IntoSet` contributions to the same element type are **never** flagged as duplicates.
3. Codegen emits one factory per `Set<T>` key — `getSetOfPlugin(): Set<Plugin>` — whose body is `new Set([PluginsModule.auth(), PluginsModule.logging()])`. Singleton scope on the multibinding cachs the constructed `Set<T>` itself; per-element scope is unaffected.

Out of scope for v0.1: `@IntoMap` / `@StringKey`, `@IntoSet` on `@Binds` (rejected by `IntoSetWithoutProvides`), `@IntoSet` on `@Inject` ctors, and child-subcomponent contributions to a parent's `Set<T>`.

## `ParsedFile`

Everything a single `.ts` file contributes to the IR. Produced by `anvil_parser::parse_file` (or `parse_source`) and aggregated across files by the CLI before being handed to `anvil-core`'s graph builder.

```rust
pub struct ParsedFile {
    pub path: String,                          // source path, informational
    pub modules: Vec<ModuleDecl>,              // @Module classes in this file
    pub components: Vec<ComponentDecl>,        // @Component classes in this file
    pub subcomponents: Vec<SubcomponentDecl>,  // @Subcomponent classes in this file (M8)
    pub inject_classes: Vec<Binding>,          // self-bindings from @Inject ctors
}
```

Each `@Inject`-annotated constructor produces a `Binding` whose `Key` is the class itself and whose `Provider` is `InjectCtor`. The class's `Scope` is taken from a co-located `@Singleton` decorator on the class, defaulting to `Unscoped`.

## Worked example

### Source
```ts
// src/coffee/heater.ts
@Inject
@Singleton
export class Heater { constructor() {} }

// src/coffee/pump.ts
@Inject
export class Pump { constructor(private heater: Heater) {} }

// src/coffee/coffee-component.ts
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
  abstract heater(): Heater;
}
```

### IR

```rust
// One ComponentDecl
ComponentDecl {
    class: ClassRef { module: "/abs/coffee-component.ts", name: "CoffeeShop" },
    modules: vec![],
    scope: Scope::Unscoped,
    entry_points: vec![
        EntryPoint { name: "pump",   key: Key::Class { /*pump.ts*/ "Pump" } },
        EntryPoint { name: "heater", key: Key::Class { /*heater.ts*/ "Heater" } },
    ],
}

// Two implicit @Inject ctor bindings (gathered from the import closure)
Binding {
    key: Key::Class { /*pump.ts*/ "Pump" },
    provider: Provider::InjectCtor { class: ClassRef { /*pump.ts*/ "Pump" } },
    scope: Scope::Unscoped,
    deps: vec![ Key::Class { /*heater.ts*/ "Heater" } ],
}
Binding {
    key: Key::Class { /*heater.ts*/ "Heater" },
    provider: Provider::InjectCtor { class: ClassRef { /*heater.ts*/ "Heater" } },
    scope: Scope::Singleton,
    deps: vec![],
}
```

The codegen turns this into [the file shown in `codegen.md`](./codegen.md#worked-example).
