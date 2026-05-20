# 0005 — Plugin Architecture for NestJS-like Controllers

**Status:** Superseded by ADR-0006
**Date:** 2026-05-12

## Context

A significant fraction of user-reported demand explicitly cites legacy decorator support and NestJS interop (`@Controller`, `@Get`, etc.). `anvil` is fundamentally a dependency injection compiler, not a full-blown web framework. However, users migrating from NestJS want the ergonomic, decentralized routing where controllers self-register via decorators, without manually mapping each route in their Express application.

Currently, `anvil`'s CLI architecture supports Wasm plugins that receive a `ParsedFile` and return a `Vec<Binding>`. This design is insufficient for NestJS-like controllers for two main reasons:

1. **Method Metadata Filtering:** `anvil-parser` only extracts known Stage-3 DI decorators on classes and methods. Custom method decorators like `@Get('/users')` are discarded during lowering.
2. **Provider Limitations:** A Wasm plugin can only return `Binding`s utilizing existing `Provider` variants (`InjectCtor`, `ProvidesMethod`, etc.). It needs a way to synthesize custom factory logic (to wire `Express` `Request`/`Response` into request-scoped controllers) without polluting the intermediate representation with raw, unverified TypeScript strings.

Three approaches are viable. They trade off ergonomics and self-registration against IR complexity, plugin lifecycle changes, and coupling to anvil's internals.

## Approach A — Synthetic `@Provides` Module

The plugin writes a generated `.ts` file during `extract_bindings`, using structured data already available in the sidecar (class names, resolved deps, decorator args). No new IR types or codegen callbacks are required.

### How it works

Given a user-written controller:

```typescript
@Controller('/users')
@Inject
export class UserController {
  constructor(private repo: UserRepository) {}

  @Get('/:id')
  byId(req: Request, res: Response) { ... }
}
```

The plugin emits `routes.module.ts` alongside it:

```typescript
import { Module, Provides, IntoSet } from "@anvil-di/anvil";
import { UserController } from "./user-controller";
import { RouteDefinition } from "@anvil-di/anvil-plugin-nestjs";

@Module
export class RoutesModule {
  @IntoSet @Provides
  static userControllerGetById(controller: UserController): RouteDefinition {
    return {
      method: "GET",
      path: "/users/:id",
      handler: (req, res) => controller.byId(req.params.id),
    };
  }
}
```

The user adds `RoutesModule` to their `@Component`'s `modules` list. Anvil's standard parse → graph → codegen pipeline handles everything from there.

### What it avoids

- No new `Provider` variants in `anvil-core`.
- No `generate_factory` codegen callback.
- No two-phase plugin lifecycle — the plugin instance is not kept alive past `extract_bindings`.
- No `Binding.metadata` field required for this use case.

### Drawbacks

- **Breaks self-registration.** The user must manually add `RoutesModule` to their `@Component`. NestJS's central appeal is that controllers register themselves. A generated aggregator module could close this gap but adds another generated-file layer.
- **Generated file is also a build input.** Unlike `.anvil.ts` daggers (outputs only), `routes.module.ts` is both a codegen output and an input to the next anvil pass. If the user runs `tsc` without running `anvil build` first, they get confusing `Cannot find module` errors from tsc rather than a clear anvil diagnostic.
- **Watch mode needs updating.** M5's file-closure tracking records which source files are *inputs* to codegen. It currently has no concept of files that codegen produces as inputs. The watcher would need to detect changes to `@Controller` files and re-trigger synthesis of `routes.module.ts` before the normal build pass.
- **Migration fit only.** Because of the manual `RoutesModule` registration step, this approach suits a transitional migration shim better than a permanent ergonomic feature. Greenfield users expecting NestJS-style self-registration will find it awkward.

---

## Approach C — Standalone Pre-Codegen Tool

A completely separate CLI (`anvil-nestjs-codegen` or similar) runs before anvil in the build pipeline. It reads `@Controller`/`@Get` decorators from TypeScript source, generates standard `@Module`/`@Provides` files, and exits. Anvil then runs normally on the result with no knowledge that a pre-pass occurred.

```
anvil-nestjs-codegen --entry src/   # writes routes.module.ts
anvil build --entry src/app.ts      # reads routes.module.ts like any other @Module
tsc --noEmit
```

### What it avoids

This is Approach A's synthetic module idea taken to its conclusion: the tool is not hosted inside anvil at all. There is no Wasm boundary, no `PluginClassInfo` sidecar, no `DecoratorArg` enum, no changes to anvil's IR or CLI. The tool can be written in any language and published independently. It reads raw `.ts` files with its own parser and writes raw `.ts` files — the interface between the two tools is the filesystem.

Because the tool is decoupled from anvil's release cycle, it has no ABI stability constraint. Every time anvil's IR evolves, Approach A and B plugins must either update to the new sidecar types or wait for a compatibility shim. A standalone tool has no such dependency: it does not import any anvil crates and is not affected by anvil's internal changes.

### Non-literal arguments

This is where Approach C has a genuine advantage over A and B. Both plugin approaches must reject non-literal decorator arguments because they are hosted inside anvil's pipeline, which deliberately avoids invoking a type checker. A standalone tool has no such constraint — it can optionally invoke `tsc`, use ts-morph, or apply the TypeScript compiler API to resolve `BASE` in `@Controller(BASE)` if it chooses to. That capability is the tool's own concern, not anvil's. Users who need non-literal support can use Approach C; users who don't can use A or B.

### Drawbacks

The drawbacks are the same as Approach A, plus an additional one from the external positioning:

- **Ordering is the user's responsibility.** There is no mechanism to enforce that `anvil-nestjs-codegen` runs before `anvil build`. The user must wire this into their `package.json` scripts, CI, or build tool. Forgetting produces confusing downstream errors from anvil or tsc rather than a diagnostic from the tool that should have run first.
- **Watch mode requires external composition.** Anvil's M5 watcher tracks source file closures and rebuilds affected components. A standalone tool has no visibility into this. Getting watch mode to work requires composing two watchers: the pre-codegen tool's watcher regenerates `routes.module.ts` on changes to `@Controller` files, which anvil's watcher then detects and uses to trigger a rebuild. That composition does not exist today.
- **Generated files have ambiguous ownership.** `routes.module.ts` lives alongside user code and could be accidentally edited. This is the same issue as Approach A, but more visible because there is no plugin host taking responsibility for the file.
- **Self-registration is not achievable.** Same as Approach A — the user must manually add the generated module to their `@Component`.

### Integration via `anvil-unplugin`

The ordering and watch mode problems are best solved not by making the standalone tool a full citizen of anvil's pipeline, but by integrating it as a pre-build hook in `anvil-unplugin`, which already owns build orchestration in bundler contexts:

```typescript
// vite.config.ts
import anvil from '@anvil-di/anvil-unplugin/vite';
import { nestjsCodegen } from '@anvil-di/anvil-plugin-nestjs/codegen';

export default {
  plugins: [
    anvil({
      mode: 'native',
      preBuild: [nestjsCodegen({ entry: 'src/' })],
    }),
  ],
};
```

The unplugin runs `preBuild` hooks in order before invoking `anvil build`, enforcing ordering automatically. It also receives file-change events from the bundler's own watcher, giving pre-build hooks enough information to decide whether to re-run. This eliminates the ordering and watch mode drawbacks within bundler contexts, though bare `anvil build` in scripts or CI still requires the user to chain commands manually.

---

## Approach B — Two-Phase Plugin with Opaque Metadata (Proposed)

Minimal enhancements to the parser and IR empower plugins to participate in both the extraction and codegen phases, preserving self-registration and keeping the IR declarative.

### 1. Extensible Decorator Extraction

Plugins export a manifest declaring the decorators they process:

```json
{ "class_decorators": ["Controller"], "method_decorators": ["Get", "Post"] }
```

When `anvil-parser` encounters these, it resolves the class's constructor parameters into `Key`s (as it does for `@Inject`) and packages the class, its resolved deps, and raw decorator arguments into a typed sidecar:

```rust
pub struct PluginClassInfo {
    pub class: ClassRef,
    pub deps: Vec<Key>,
    pub decorator_args: Vec<DecoratorArg>,
    pub methods: Vec<PluginMethodInfo>,
}

pub struct PluginMethodInfo {
    pub name: String,
    pub decorator_args: Vec<DecoratorArg>,
}

pub enum DecoratorArg {
    /// A string, number, or boolean literal — usable directly by the plugin.
    Literal(serde_json::Value),
    /// Any argument that cannot be reduced to a literal at parse time
    /// (identifier references, template literals, function calls, member
    /// expressions). The raw source text is preserved for diagnostics only;
    /// plugins must reject bindings that depend on a `NonLiteral` argument.
    NonLiteral { source: String },
}
```

This sidecar is passed to the plugin alongside the `ParsedFile`.

#### Constraint: decorator arguments must be statically knowable

Anvil does not invoke a type checker or evaluate expressions. When a decorator argument is not a literal, its value cannot be known at parse time:

```typescript
const BASE = '/users';

@Controller(BASE)           // identifier reference
@Controller(`${BASE}/:id`)  // template literal
@Controller(buildPath())    // function call
@Get(ROUTES.GET_BY_ID)      // member expression
```

All four cases produce `DecoratorArg::NonLiteral`. This is consistent with anvil's broader invariant (ADR-0003): rather than silently pass through an unresolvable value and produce code that compiles but behaves incorrectly, or defer to a type checker that anvil deliberately avoids, the plugin must reject any binding whose routing metadata depends on a non-literal argument.

Passing `NonLiteral::source` through to `generate_factory` and emitting it verbatim is explicitly ruled out. The generated expression would pass the `oxc_parser` structural gate (it is syntactically valid TypeScript) but fail `tsc --noEmit` because the identifier is almost certainly not in scope in the dagger's output file. That failure surfaces at typecheck time rather than at anvil build time, which is precisely the class of confusing error this architecture is designed to prevent.

The `anvil-plugin-nestjs` plugin must therefore emit an `ExtractError`-equivalent diagnostic for any `@Controller` or `@Get`/`@Post` argument that is `NonLiteral`, with a message directing the user to use a string literal. This is a known ergonomic gap relative to NestJS, which evaluates these arguments at runtime.

### 2. Plugin-Emitted Core Providers

With resolved deps available in the sidecar, the plugin emits a `Provider::InjectCtor` binding for the controller class and attaches its metadata:

```rust
Binding {
    key: Key::Class { name: "UserController", .. },
    provider: Provider::InjectCtor { deps: vec![user_repo_key] },
    metadata: BTreeMap::from([
        ("anvil:plugin".into(), json!({ "id": "nestjs", "base_path": "/users" }))
    ]),
    ..
}
```

To prevent double-binding when the user also writes `@Inject`, classes claimed by a plugin manifest are skipped during standard extraction. The manifest's `"suppresses"` list controls this:

```json
{ "class_decorators": ["Controller"], "suppresses": ["Controller"] }
```

### 3. Opaque Metadata

`Binding.metadata: BTreeMap<String, serde_json::Value>` is added to `anvil_core::ir::Binding`. `anvil-core` treats it as fully opaque — it is preserved through graph building and validation but never interpreted. This is the only change to `anvil-core`.

> **Note:** `Provider::Plugin` is intentionally **not** added to `anvil-core`. Adding a plugin-sentinel provider variant would force every `match` on `Provider` across core, codegen, and CLI to carry a plugin-aware arm, violating the boundary that core is pure data + rules. Plugin-owned bindings instead use existing provider variants (`InjectCtor`, `ProvidesMethod`) and are identified at codegen time by the `"anvil:plugin"` key in `metadata`.

### 4. `generate_factory` Hook

`anvil-codegen` detects `binding.metadata.contains_key("anvil:plugin")` and calls a new Wasm export on the corresponding plugin:

```rust
generate_factory(binding: Binding) -> String
```

The returned string must be a valid TypeScript expression (not a full statement); codegen wraps it in a minimal parseable scaffold, runs it through `oxc_parser` + `oxc_codegen`, and rejects it on parse failure — preserving the existing invariant that no unparsed TS is ever written to disk.

This requires the CLI to keep plugin instances alive from the parse phase through codegen. The current plugin host (`crates/anvil-cli/src/plugin.rs`) drops the plugin after `extract_bindings`; it must be extended to maintain a `plugin_id → PluginRunner` map across both phases.

### 5. Implement the `@anvil-di/anvil-plugin-nestjs` Wasm Plugin

1. **Extraction (`extract_bindings`):**
   - Emits `Provider::InjectCtor` for each `@Controller` class with its resolved deps and base-path metadata.
   - For each `@Get`/`@Post`/… method, synthesizes an `@IntoSet` contribution binding targeting `Set<RouteDefinition>` using `Provider::InjectCtor` + route metadata.

2. **Codegen (`generate_factory`):** Reads `metadata["anvil:plugin"]` and returns the adapter expression:

   ```typescript
   {
     method: 'GET',
     path: '/users/:id',
     handler: (req, res) => {
       const controller = this.requestComponent(req, res).getUserController();
       controller.byId(req.params.id);
     }
   }
   ```

### 6. App Integration

```typescript
@Inject
export class Router {
  constructor(private routes: Set<RouteDefinition>) {}

  register(app: express.Application) {
    for (const route of this.routes) {
      app[route.method.toLowerCase()](route.path, route.handler);
    }
  }
}
```

`RouteDefinition` is defined in `@anvil-di/anvil-plugin-nestjs` and is a dev dependency (type-only import in user code).

---

## Comparison

| | Approach A — Synthetic Module | Approach B — Two-Phase Plugin | Approach C — Standalone Tool |
|---|---|---|---|
| IR changes | None | `Binding.metadata` field only | None |
| New `Provider` variant | No | No (explicitly avoided) | No |
| Coupling to anvil internals | Wasm ABI, sidecar types | Wasm ABI, sidecar types, `Binding.metadata` | None — filesystem only |
| Plugin / tool lifecycle | Single-phase, drop after extract | Must survive parse → codegen | Separate process, no shared state |
| Self-registration | No — user adds module manually | Yes — controller self-registers | No — user adds module manually |
| Watch mode | Needs generated-input tracking | No change required | External composition required |
| Build ordering enforced by | anvil plugin host | anvil plugin host | User scripts / `preBuild` hook |
| Generated files | `routes.module.ts` (input + output) | `.anvil.ts` daggers only | `routes.module.ts` (input + output) |
| `generate_factory` string validation | N/A | oxc_parser gate required | N/A |
| Non-literal decorator args | Rejected with diagnostic | Rejected with diagnostic | Optionally resolvable via tsc/ts-morph |
| Migration fit | Good | Good | Good |
| Greenfield fit | Poor | Good | Poor |
| Independent release cycle | No | No | Yes |

**Recommendation:** Approach B for any use case where self-registration ergonomics matter or where a long-lived, first-class feature is the goal. Approach C for a migration shim where the ability to handle non-literal decorator arguments is important, or where complete decoupling from anvil's release cycle is a hard requirement — ideally integrated via the `anvil-unplugin` `preBuild` hook to recover the ordering and watch mode benefits. Approach A is the weakest choice: it carries Approach C's drawbacks (generated input files, manual module registration) without Approach C's benefits (no ABI coupling, non-literal support, independent versioning).

---

## Consequences

**Good:**
- **Separation of Concerns:** `anvil-core` remains ignorant of HTTP semantics.
- **Clean IR:** The intermediate representation stays pure and declarative. No raw code blocks pass through `anvil-core`.
- **Inter-Plugin Communication:** Opaque metadata on the IR can be read by other plugins in the pipeline.
- **Safe Validations:** `anvil-core` validates all dependency edges accurately before any custom code is generated. `generate_factory` is never called on an invalid graph.

**Bad (Approach B):**
- **Two-phase plugin invocation:** Marginally increases build time. More significantly, requires the CLI to maintain plugin instances across the parse and codegen phases — a non-trivial change to the current plugin host.
- **`generate_factory` string validation:** The oxc_parser gate is necessary but adds a failure mode that surfaces at codegen time rather than at extraction time.

## Alternatives Considered

- **Inline Factory Strings in IR:** The plugin emits `Provider::InlineFactory { body: "..." }` during extraction. *Rejected* because it pollutes the IR with raw TypeScript strings, breaking the declarative nature of the graph.
- **`Provider::Plugin` sentinel variant:** Adding a plugin-sentinel to `anvil-core::ir::Provider` to trigger codegen callbacks. *Rejected* because it forces every `match` on `Provider` across the codebase to carry a plugin-aware arm, violating core's boundary as pure data + rules.
- **Passing `NonLiteral::source` through to `generate_factory`:** The plugin stores the raw source text and emits it verbatim in the generated expression. *Rejected* because the emitted code passes the `oxc_parser` structural gate but fails `tsc --noEmit` when the identifier is not in scope in the dagger's output file. The failure surfaces at typecheck time rather than at anvil build time, which is the class of confusing diagnostic this architecture is designed to prevent.
- **Standalone pre-codegen tool (Approach C):** Not rejected — documented above as a viable path for migration use cases, particularly where non-literal decorator argument support or an independent release cycle is required. Weaker than Approach B for greenfield use because self-registration and seamless watch mode require external composition.
- **Macro System / Source Rewriting:** Plugins rewrite the user's `.ts` AST before lowering. *Rejected* due to risks of breaking source maps and line diagnostics.
