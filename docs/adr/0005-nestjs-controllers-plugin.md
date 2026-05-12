# 0005 — Plugin Architecture for NestJS-like Controllers

**Status:** Proposed
**Date:** 2026-05-12

## Context

A significant fraction of user-reported demand explicitly cites legacy decorator support and NestJS interop (`@Controller`, `@Get`, etc.). `anvil` is fundamentally a dependency injection compiler, not a full-blown web framework. However, users migrating from NestJS want the ergonomic, decentralized routing where controllers self-register via decorators, without manually mapping each route in their Express application.

Currently, `anvil`'s CLI architecture supports Wasm plugins that receive a `ParsedFile` and return a `Vec<Binding>`. This design is insufficient for NestJS-like controllers for two main reasons:

1. **Method Metadata Filtering:** `anvil-parser` only extracts known Stage-3 DI decorators on classes and methods. Custom method decorators like `@Get('/users')` are discarded during lowering.
2. **Provider Limitations:** A Wasm plugin can only return `Binding`s utilizing existing `Provider` variants (`InjectCtor`, `ProvidesMethod`, etc.). It needs a way to synthesize custom factory logic (to wire `Express` `Request`/`Response` into request-scoped controllers) without polluting the intermediate representation with raw, unverified TypeScript strings.

## Decision / Proposal

We propose minimal enhancements to the parser and IR to empower plugins, emphasizing a pure, declarative intermediate representation via opaque metadata:

### 1. Extensible Decorator Extraction
Instead of hardcoding decorator aliases or introducing a heavy intermediate AST node, Wasm plugins will export a manifest declaring the decorators they process.
- e.g., `{"class_decorators": ["Controller"], "method_decorators": ["Get", "Post"]}`
- When `anvil-parser` encounters these, it performs the standard DI extraction (e.g., resolving the class's constructor parameters into `Key`s, just as it does for `@Inject`). 
- It then passes this packaged information—the class, its resolved constructor dependencies, and the raw decorator arguments (like `'/users'`)—to the plugin as a lightweight sidecar alongside the `ParsedFile`.

### 2. Plugin-Emitted Core Providers
With the sidecar providing the resolved constructor dependencies, the plugin can take full control of the class binding. It doesn't just synthesize new factories; it directly emits the `Provider::InjectCtor` binding for the controller class. This allows the plugin to attach its custom base-path metadata directly to the class binding:
- e.g., The plugin emits `Provider::InjectCtor`, attaching `metadata: {"nestjs": {"base_path": "/users"}}`.

### 3. Opaque Metadata & Plugin Providers
To pass information from the parsing phase to the codegen phase without embedding code strings in the IR, we introduce namespaced metadata and a new provider type:
- **`Binding.metadata`:** Add `metadata: BTreeMap<String, serde_json::Value>` to `anvil_core::ir::Binding`. `anvil-core` treats this as opaque data.
- **`Provider::Plugin`:** Add `Provider::Plugin { plugin_id: String }` to the IR. This signals that a plugin must generate the final code for this binding.
- **`generate_factory` Hook:** `anvil-codegen` will invoke a new Wasm export (`generate_factory(Binding) -> String`) on the corresponding plugin, allowing it to emit the TypeScript implementation using its own stored metadata.

### 4. Implement the `@msulak/anvil-plugin-nestjs` Wasm Plugin
The plugin will read the method metadata sidecar and operate in two phases:
1. **Extraction Phase (`extract_bindings`):** 
   - The plugin emits a `Provider::InjectCtor` binding for the controller class, populated with the resolved dependencies from the sidecar and attaching the `@Controller` base path to `metadata["nestjs"]`.
   - For every method decorated with `@Get`, `@Post`, etc., it synthesizes an `@IntoSet` contribution binding targeting `Set<RouteDefinition>`. It uses `Provider::Plugin` and attaches the route details to `metadata["nestjs"]`.
2. **Codegen Phase (`generate_factory`):** When `anvil-codegen` encounters this binding, it calls the plugin back. The plugin reads `metadata["nestjs"]` and returns the adapter function:
   ```typescript
   return {
     method: 'GET',
     path: '/users/:id',
     handler: (req, res) => {
       const controller = this.requestComponent(req, res).userController();
       controller.byId(req.params.id);
     }
   };
   ```

### 5. App Integration
The user provides an Express bootstrap module that injects the aggregate `Set<RouteDefinition>` and registers them:
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

## Consequences

**Good:**
- **Separation of Concerns:** `anvil-core` remains ignorant of HTTP semantics.
- **Clean IR:** The intermediate representation stays pure, declarative, and language-agnostic. No raw code blocks are sent through `anvil-core`.
- **Inter-Plugin Communication:** Opaque metadata attached to the IR could theoretically be read by other plugins in the pipeline.
- **Safe Validations:** `anvil-core` can validate all dependency edges accurately before any custom code is generated.

**Bad:**
- **Wasm Overhead:** Requires a two-phase plugin invocation (extraction during `anvil-cli check/build`, and generation during `anvil-codegen`), marginally increasing build times compared to a single pass.

## Alternatives considered

- **Inline Factory Strings in IR:** The plugin emits `Provider::InlineFactory { body: "..." }` directly during extraction. *Rejected* because it pollutes the intermediate representation with raw TypeScript strings, breaking the declarative nature of the graph and making it harder for tooling to analyze bindings.
- **Zero IR Change (WASI Read/Write):** To avoid changing the IR, the plugin uses WASI to parse `.ts` files, extract decorators, and write physical `routes.module.ts` files containing standard `@Provides` methods. *Rejected* because bundling a TS parser bloats Wasm size, and generating intermediate physical files degrades the developer experience.
- **Macro System / Source Rewriting:** Plugins rewrite the user's `.ts` AST before lowering. *Rejected* due to risks of breaking source maps and line diagnostics.