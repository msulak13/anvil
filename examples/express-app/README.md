# `examples/express-app`

A small Express server demonstrating tsdi's per-request DI scopes, built around `@Subcomponent` factory parameters (M11). Express's `Request` and `Response` flow into a request-scoped graph as runtime values; everything else (logger, repository) stays app-scoped on the parent component.

## Layout

| File | Role |
|---|---|
| `src/logger.ts`, `src/console-logger.ts` | `Logger` interface + `@Singleton @Inject` impl |
| `src/user-repository.ts` | `@Singleton @Inject` in-memory user store |
| `src/app-module.ts` | `@Module` aliasing `Logger` → `ConsoleLogger` via `@Binds` |
| `src/request-context.ts` | Per-request derived state (path, method, requestId, userId) |
| `src/request-module.ts` | `@Module` whose `@Provides static context(req: Request)` builds a `RequestContext` from the factory-param `req` |
| `src/user-controller.ts` | `@Inject` controller; consumes `RequestContext` + `Response` (request-scoped) **and** `Logger` + `UserRepository` (app-scoped, inherited) |
| `src/request-component.ts` | `@Subcomponent` exposing `userController()` and `context()` |
| `src/app-component.ts` | `@Singleton @Component` with `requestComponent(req: Request, res: Response): RequestComponent` |
| `src/server.ts` | Express app: builds the dagger once, threads each request through `dagger.requestComponent(req, res)` |
| `src/server.test.ts` | Vitest + supertest end-to-end tests proving per-request scope works |

## Running

```bash
pnpm install
pnpm --filter express-app start          # start the server (port 3000 by default)
pnpm --filter express-app test           # run the supertest suite
pnpm --filter express-app typecheck      # tsc --noEmit
```

The `start` and `test` scripts invoke `tsdi build` first to (re)generate `src/app-component.tsdi.ts` from the decorated source.

## What the dagger emits

Running `tsdi build --entry src/app-component.ts` produces `src/app-component.tsdi.ts` containing:

```ts
class DaggerAppComponent extends AppComponent {
  private _consoleLogger: ConsoleLogger | undefined;
  private _userRepository: UserRepository | undefined;
  // … cached singletons …
  requestComponent(req: Request, res: Response): RequestComponent {
    return new DaggerRequestComponent(this, req, res);   // ← M11
  }
}

class DaggerRequestComponent extends RequestComponent {
  constructor(
    private parent: DaggerAppComponent,
    private req: Request,
    private res: Response,
  ) { super(); }

  private getRequest(): Request   { return this.req; }
  private getResponse(): Response { return this.res; }
  private getRequestContext(): RequestContext {
    return RequestModule.context(this.getRequest());
  }
  private getUserController(): UserController {
    return new UserController(
      this.getRequestContext(),       // request-scoped
      this.getResponse(),             // factory param
      this.parent.getLogger(),        // inherited app-scoped
      this.parent.getUserRepository(),// inherited app-scoped
    );
  }
  // … entry points …
}
```

Each HTTP request gets a fresh `DaggerRequestComponent`, so `RequestContext` is built fresh per request from that request's `req`. Singletons stay on the parent and are shared across every child.

## What this proves

1. **Request scope without `AsyncLocalStorage` or middleware globals.** The `req` and `res` flow through the type system into the dagger.
2. **Lifetime correctness is checked at compile time.** A `@Singleton @Subcomponent` taking factory params is rejected by `tsdi check` (the cache would freeze the first call's args).
3. **Inheritance works automatically.** The user code never says "this binding lives on the parent" — the graph layer detects that `Logger` / `UserRepository` are unsatisfiable inside the child and routes their getters through `this.parent.getX()`.
