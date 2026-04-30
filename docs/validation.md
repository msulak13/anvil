# Validation rules

The rules `tsdi-core::graph::build_and_validate` enforces over the dependency graph. Each rule has a corresponding `DiagnosticKind` variant in [`crates/tsdi-core/src/validate.rs`](../crates/tsdi-core/src/validate.rs) and a CLI integration test in [`crates/tsdi-cli/tests/check_command.rs`](../crates/tsdi-cli/tests/check_command.rs).

Update this page whenever a rule is added, refined, or relaxed.

## Status

| Rule              | Implemented in | Variant                               |
| ----------------- | -------------- | ------------------------------------- |
| `MissingBinding`                     | M3  | `DiagnosticKind::MissingBinding`                     |
| `Cycle`                              | M3  | `DiagnosticKind::Cycle`                              |
| `Duplicate`                          | M3  | `DiagnosticKind::Duplicate`                          |
| `ScopeMismatch`                      | M3  | `DiagnosticKind::ScopeMismatch`                      |
| `FactoryParamsOnNonSubcomponentEntry` | M11 | `DiagnosticKind::FactoryParamsOnNonSubcomponentEntry` |
| `DuplicateFactoryParam`              | M11 | `DiagnosticKind::DuplicateFactoryParam`              |
| `SingletonSubcomponentWithFactoryParams` | M11 | `DiagnosticKind::SingletonSubcomponentWithFactoryParams` |
| `AsyncBindingNeedsSingletonComponent`    | M12 | `DiagnosticKind::AsyncBindingNeedsSingletonComponent`    |

Multibinding-specific validation (`@IntoSet` on `@Binds` or with no `@Provides`) is enforced by the parser as `ExtractError::IntoSetWithoutProvides` rather than as a graph diagnostic — it's a structural problem, not a graph problem.

## Producing diagnostics

`tsdi-core` is I/O-free: `build_and_validate(GraphInput)` returns a `(DependencyGraph, Vec<Diagnostic>)` pair. Each `Diagnostic` carries:

- a [`DiagnosticKind`](../crates/tsdi-core/src/validate.rs) discriminator,
- a `primary: Label` (the location to anchor the error on),
- zero or more `related: Vec<Label>` (notes pointing at second declarations, cycle members, the enclosing component, etc.).

Each `Label` is a `(SourceSpan, message)` pair. `SourceSpan { path, start, end }` uses absolute paths supplied by the M2 resolver, so multi-file diagnostics work.

The CLI's [`crates/tsdi-cli/src/diagnostics.rs`](../crates/tsdi-cli/src/diagnostics.rs) module reads each diagnostic's primary file from disk and renders the diagnostic as a `miette::Report` with inline snippets. Labels in *other* files become trailing help-text notes — the same convention `rustc` uses for cross-file errors.

## `MissingBinding`

> A binding was requested for which no provider exists.

### Triggering input
```ts
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;   // Pump's @Inject ctor needs a Heater, but no Heater is reachable
}
```

### Diagnostic shape
```
error: missing binding for Heater (requested by Pump)
  ┌─ src/coffee/pump.ts:5:31
  │
5 │   constructor(private heater: Heater) {}
  │                               ^^^^^^ no provider declares this key
  │
  = help: ensure a class with class-level @Inject is reachable, or add a @Provides method
```

### Detection
Walk every `EntryPoint` in the `ComponentDecl` and traverse the `deps` of each binding. The first key that has no matching `Binding` triggers the error. The `requested_by` field carries the parent key for context.

## `Cycle`

> The dependency graph contains a cycle.

### Triggering input
```ts
@Inject export class A { constructor(private b: B) {} }
@Inject export class B { constructor(private a: A) {} }
```

### Diagnostic shape
```
error: dependency cycle
  → A
  → B
  → A   (cycle closes here)
```

### Detection
Tarjan's strongly-connected-components algorithm via `petgraph::algo::tarjan_scc`, run after the graph is fully populated. Any SCC of size > 1 (or a self-loop) yields a `Cycle` diagnostic containing the keys in traversal order. Implemented in `tsdi-core::graph::detect_cycles`.

## `Duplicate`

> Two or more bindings declare the same `Key`.

### Triggering input
```ts
@Module
export class ModuleA { @Provides static heater(): Heater { return new Heater(); } }

@Module
export class ModuleB { @Provides static heater(): Heater { return new Heater(); } }

@Component({ modules: [ModuleA, ModuleB] })
export abstract class Shop { abstract heater(): Heater; }
```

### Diagnostic shape
```
error: duplicate binding for Heater
  declared at src/module-a.ts:2:3
   and at  src/module-b.ts:2:3
  = help: remove one of the @Provides methods, or differentiate via @Binds (M7+)
```

### Detection
While collecting bindings into the graph (`aggregate_bindings`), each `Key` records the spans of every contributor in a `HashMap<Key, Vec<SourceSpan>>`. Any key with > 1 source produces one `Duplicate` diagnostic listing all sites. **Multibindings (`@IntoSet`) bypass this rule** (M9): raw bindings carrying `MultibindRole::IntoSet` are routed through a separate aggregation map and never enter the duplicate-tracking `sources_for_key`. Multiple `@IntoSet` contributions to the same element type are intentional, not duplicates.

## `ScopeMismatch`

> A `Singleton`-scoped binding sits inside a non-`Singleton` component.

In v0.1 we only enforce one direction: a binding declared `@Singleton` requires the enclosing component to also be `@Singleton`-scoped. The richer Dagger-style transitive-scope rule (a `@Singleton` provider may not depend on an `Unscoped` one) is deferred until the codegen actually emits scoped caches in M6.

### Triggering input
```ts
@Inject
@Singleton
export class Heater { constructor() {} }

@Component({ modules: [] })   // <-- not @Singleton
export abstract class Comp { abstract heater(): Heater; }
```

### Diagnostic shape
```
error[tsdi::scope_mismatch]: scope mismatch on Heater@/p/heater.ts: binding is Singleton but component is Unscoped
  ┌─ src/heater.ts
  │
  │   @Inject
  │   @Singleton
  │   ^^^^^^^^^^ singleton binding
  │
  = help: component is not @Singleton (at src/comp.ts:...)
```

### Detection
`tsdi-core::graph::detect_scope_mismatches` walks the aggregated bindings once and emits one diagnostic per offending key. Skipped entirely when the component is `Singleton`.

### Subcomponents (M8)
Each `@Subcomponent` reachable from a `@Component`'s entry points runs `detect_scope_mismatches` against *its own* scope and *its own* local bindings only — bindings inherited from the parent are not re-validated, since the parent already validated them under its own scope. Missing-binding and cycle checks run against the child graph **after** parent fallback: a child dep that only the parent provides is satisfied (no `MissingBinding`) and contributes an inherited-key edge that doesn't participate in cycle detection on the child side. A cycle that crosses the parent/child boundary is a v0.2 limitation — current detection runs per-graph.

## How diagnostics are rendered

Diagnostics flow through two layers:

1. **`tsdi-core`** emits structured `Diagnostic` values. No I/O, no formatting — pure data.
2. **`tsdi-cli`** ([`src/diagnostics.rs`](../crates/tsdi-cli/src/diagnostics.rs)) loads the primary span's source file and builds a `miette::MietteDiagnostic` with `LabeledSpan`s for every label in that file. Labels in other files are appended as `help` notes.

Integration tests in [`crates/tsdi-cli/tests/check_command.rs`](../crates/tsdi-cli/tests/check_command.rs) materialize fixture projects in tempdirs and assert that each diagnostic's summary appears on stderr with the right exit code (`1` for diagnostic output, `2` for tooling errors).
