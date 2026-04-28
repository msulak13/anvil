# IR specification

The internal representation of the binding graph. This is the **stable contract** between `tsdi-parser` and `tsdi-codegen`. Definitions live in [`crates/tsdi-core/src/ir.rs`](../crates/tsdi-core/src/ir.rs).

Update this page whenever a variant is added or removed.

## `Key`

Stable identity for a TypeScript type without a full type checker. Two values are equal iff they refer to the same exported declaration in the same source file.

```rust
pub enum Key {
    Class { module: ModulePath, name: String },
    // Token { module: ModulePath, name: String },  // v0.2 / M7
}

pub struct ModulePath(pub String);  // M1: raw import specifier; M2+: absolute, normalized path
```

In **M1** the parser stores the *raw import specifier* in `ModulePath` (e.g. `"./heater"`, `"tsdi"`, `"my-pkg/sub"`). M2's cross-file resolver rewrites these to absolute paths so equivalent imports compare equal. For type identifiers declared in the same file as their reference, the parser uses the sentinel `ModulePath::SAME_FILE` (`"<self>"`); M2 swaps it for the file's actual absolute path.

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
    InjectCtor     { class: ClassRef },
    ProvidesMethod { module: ClassRef, method: String },
    Binds          { target: Key },
}
```

- **`InjectCtor`** — class with class-level `@Inject` decorator. The constructor's parameter types become the binding's `deps`. The codegen emits `new ClassName(deps...)`.
- **`ProvidesMethod`** — static method on a `@Module`. The codegen emits `ModuleName.methodName(deps...)`.
- **`Binds`** (M7) — alias binding. The owning `@Module` exposes a `static` method whose single parameter type is the implementation and whose return type is the alias. The binding's `key` is the return type (the alias); `target` is the parameter type (the implementation). `deps` is `vec![target]` so the topo walk visits the target's binding before the alias's factory references it. Codegen emits `return this.<getTarget>()` — no `new` is performed by the alias factory; the target's scope governs caching.

  TC39 Stage-3 decorators cannot decorate abstract methods (TS error 1249), so `@Binds` methods are `static` with a body. `tsdi-codegen` ignores the body and emits the delegate; the body still has to compile (e.g. `return impl;`) so the user's `tsc` accepts the source file.

## `SourceSpan`

```rust
pub struct SourceSpan {
    pub path: String,   // absolute, canonical source-file path (M2+)
    pub start: u32,     // inclusive byte offset
    pub end: u32,       // exclusive byte offset
}
```

A parser-agnostic byte range used by validation diagnostics. Kept free of any `oxc_*` types so `tsdi-core` stays parser-independent. The parser converts each `oxc_span::Span` into a `SourceSpan` at extraction time (M1+); the M2 resolver rewrites every `path` field to absolute form alongside the same canonicalization it does for `ModulePath`.

## `Binding`

```rust
pub struct Binding {
    pub key: Key,
    pub provider: Provider,
    pub scope: Scope,
    pub deps: Vec<Key>,
    pub source: SourceSpan,   // M3+: where the binding appears in source
}
```

A single contribution to the graph. The `deps` are the keys the provider needs to construct its output. The `source` field anchors validation diagnostics on the right line.

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

## `ParsedFile`

Everything a single `.ts` file contributes to the IR. Produced by `tsdi_parser::parse_file` (or `parse_source`) and aggregated across files by the CLI before being handed to `tsdi-core`'s graph builder.

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
