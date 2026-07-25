import path from "node:path";
import http from "node:http";
import type { AddressInfo } from "node:net";
import express from "express";
import { describe, expect, it } from "vitest";
import { z } from "zod";
import type { ResponseCodec, Validator } from "./schema.js";
import { withJsonSchema } from "./schema.js";
import {
  Controller,
  Delete,
  Deprecated,
  Get,
  Middleware,
  Patch,
  Post,
  Put,
  Returns,
  Security,
  Tag,
} from "./decorators.js";
import { bellowsRoutes, type RouteDefinition } from "./routes.js";
import { ForbiddenError, NotFoundError } from "./errors.js";
import type { AuthnService, AuthzService } from "./authz.js";

// --- Validator<T> structural compatibility ---

describe("Validator<T>", () => {
  it("is satisfied structurally by a Zod schema", () => {
    const schema = z.object({ id: z.string() });
    // Type-level assertion: if this assignment compiles, Zod satisfies Validator<T>.
    const _v: Validator<{ id: string }> = schema;
    const result = schema.safeParse({ id: "abc" });
    expect(result.success).toBe(true);
  });
});

// --- withJsonSchema ---

describe("withJsonSchema", () => {
  const schema = z.object({ id: z.string() });
  const jsonSchema = { type: "object" as const, properties: { id: { type: "string" as const } } };

  it("delegates safeParse to the wrapped validator", () => {
    const wrapped = withJsonSchema(schema, jsonSchema);
    expect(wrapped.safeParse({ id: "hello" }).success).toBe(true);
    expect(wrapped.safeParse({ id: 42 }).success).toBe(false);
  });

  it("returns the provided JSON schema from jsonSchema()", () => {
    const wrapped = withJsonSchema(schema, jsonSchema);
    expect(wrapped.jsonSchema()).toBe(jsonSchema);
  });
});

// --- Decorator stubs are no-ops ---

describe("decorator stubs", () => {
  it("Controller returns the decorated class unchanged", () => {
    class MyController {}
    const ctx = {} as ClassDecoratorContext;
    const result = Controller("/api")(MyController as abstract new () => object, ctx);
    expect(result).toBe(MyController);
  });

  it("Get returns the decorated method unchanged", () => {
    const fn = () => "hello";
    const ctx = {} as ClassMethodDecoratorContext;
    const result = Get("/test")(fn, ctx);
    expect(result).toBe(fn);
  });

  it("Post returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Post("/test")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Put returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Put("/test")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Delete returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Delete("/test")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Patch returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Patch("/test")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Middleware returns the decorated class unchanged", () => {
    class MyController {}
    const ctx = {} as ClassDecoratorContext;
    const result = (Middleware as (...fns: never[]) => (target: typeof MyController, ctx: ClassDecoratorContext) => typeof MyController)()(MyController, ctx);
    expect(result).toBe(MyController);
  });

  it("Middleware returns the decorated method unchanged", () => {
    const fn = () => {};
    const ctx = {} as ClassMethodDecoratorContext;
    const result = (Middleware as (...fns: never[]) => (target: typeof fn, ctx: ClassMethodDecoratorContext) => typeof fn)()(fn, ctx);
    expect(result).toBe(fn);
  });

  it("Tag returns the decorated class unchanged", () => {
    class MyController {}
    const result = Tag("users")(MyController as abstract new () => object, {} as ClassDecoratorContext);
    expect(result).toBe(MyController);
  });

  it("Returns returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Returns(201, {})(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Security returns the decorated class unchanged", () => {
    class MyController {}
    const result = (Security as (scheme: string) => (target: typeof MyController, ctx: ClassDecoratorContext) => typeof MyController)("bearer")(MyController, {} as ClassDecoratorContext);
    expect(result).toBe(MyController);
  });

  it("Security returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = (Security as (scheme: string) => (target: typeof fn, ctx: ClassMethodDecoratorContext) => typeof fn)("bearer")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });

  it("Deprecated returns the decorated method unchanged", () => {
    const fn = () => {};
    const result = Deprecated("use newHandler instead")(fn, {} as ClassMethodDecoratorContext);
    expect(result).toBe(fn);
  });
});

// --- bellowsCodegen PreBuildHook ---

import { bellowsCodegen } from "./hook.js";

describe("bellowsCodegen", () => {
  it("returns a hook with the correct name", () => {
    const hook = bellowsCodegen({ entry: "src" });
    expect(hook.name).toBe("anvil-bellows-codegen");
  });

  it("watchPatterns covers .ts files under the entry directory", () => {
    const entry = path.resolve("src").replace(/\\/g, "/");
    const hook = bellowsCodegen({ entry: "src" });
    expect(hook.watchPatterns).toHaveLength(1);
    expect(hook.watchPatterns[0]).toContain(entry);
    expect(hook.watchPatterns[0]).toContain("**/*.ts");
  });

  it("shouldRerun returns true for changed .ts files under entry", () => {
    const hook = bellowsCodegen({ entry: "src" });
    const entryAbs = path.resolve("src");
    expect(hook.shouldRerun([`${entryAbs}/user-controller.ts`])).toBe(true);
  });

  it("shouldRerun returns false for .d.ts files", () => {
    const hook = bellowsCodegen({ entry: "src" });
    const entryAbs = path.resolve("src");
    expect(hook.shouldRerun([`${entryAbs}/user-controller.d.ts`])).toBe(false);
  });

  it("shouldRerun returns false for files outside the entry directory", () => {
    const hook = bellowsCodegen({ entry: "src" });
    expect(hook.shouldRerun(["/other/dir/something.ts"])).toBe(false);
  });
});

// --- bellowsRoutes: middleware chains + lifecycle hooks ---

/** Start an express app built from `routes`/`hooks` and return its base URL + a closer. */
async function serve(
  routes: RouteDefinition[],
  hooks?: Parameters<typeof bellowsRoutes>[1],
): Promise<{ url: string; close: () => Promise<void> }> {
  const app = express();
  app.use(bellowsRoutes(routes, hooks));
  const server = http.createServer(app);
  await new Promise<void>((resolve) => server.listen(0, resolve));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}

describe("bellowsRoutes", () => {
  it("runs route middleware, in order, before the handler", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        middleware: [
          (_req, _res, next) => { calls.push("first"); next(); },
          (_req, _res, next) => { calls.push("second"); next(); },
        ],
        handler: (_req, res) => { calls.push("handler"); res.json({ ok: true }); },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
      expect(calls).toEqual(["first", "second", "handler"]);
    } finally {
      await close();
    }
  });

  it("short-circuits when middleware doesn't call next()", async () => {
    let handlerCalled = false;
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        middleware: [(_req, res) => { res.status(401).json({ error: "unauthorized" }); }],
        handler: (_req, res) => { handlerCalled = true; res.json({ ok: true }); },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(401);
      expect(handlerCalled).toBe(false);
    } finally {
      await close();
    }
  });

  it("routes with no middleware still work", async () => {
    const routes: RouteDefinition[] = [
      { method: "GET", path: "/x", handler: (_req, res) => res.json({ ok: true }) },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
    } finally {
      await close();
    }
  });

  it("runs onRequest before every route's middleware and handler", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        middleware: [(_req, _res, next) => { calls.push("mw"); next(); }],
        handler: (_req, res) => { calls.push("handler"); res.json({ ok: true }); },
      },
    ];
    const { url, close } = await serve(routes, {
      onRequest: (_req, _res, next) => { calls.push("onRequest"); next(); },
    });
    try {
      await fetch(`${url}/x`);
      expect(calls).toEqual(["onRequest", "mw", "handler"]);
    } finally {
      await close();
    }
  });

  it("runs onResponse after the response has been sent", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      { method: "GET", path: "/x", handler: (_req, res) => { calls.push("handler"); res.json({ ok: true }); } },
    ];
    const { url, close } = await serve(routes, {
      onResponse: () => { calls.push("onResponse"); },
    });
    try {
      await fetch(`${url}/x`);
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(calls).toEqual(["handler", "onResponse"]);
    } finally {
      await close();
    }
  });
});

// --- ResponseCodec / Produces: the handler emitted for a `Produces<S, C>`
// route, run through a real Express server. This isn't testing anvil-bellows
// codegen itself (that's Rust-side, covered by its own unit tests) — it's
// proving that the *shape* of code codegen emits (schema.safeParse, then
// codec.contentType/encode instead of res.json) actually behaves correctly
// against a live server, not just as a string match.

describe("ResponseCodec / Produces (codegen output shape)", () => {
  const TwimlResponseSchema = z.object({
    type: z.literal("goodbye"),
    text: z.string().min(1),
  });
  type TwimlResponse = z.infer<typeof TwimlResponseSchema>;

  const twimlCodec: ResponseCodec<TwimlResponse> = {
    contentType: "application/xml",
    encode: (value) => `<Response><Say>${value.text}</Say></Response>`,
  };

  /** Exactly the handler body `anvil-bellows` codegen emits for `Produces<S, C>`. */
  function producesHandler(getResult: () => unknown): RouteDefinition["handler"] {
    return (_req, res) => {
      const result = getResult();
      const validated = TwimlResponseSchema.safeParse(result);
      if (!validated.success) {
        res.status(500).json(validated.error);
        return;
      }
      res.type(twimlCodec.contentType).send(twimlCodec.encode(validated.data));
    };
  }

  it("sends the codec-encoded body with the codec's content-type, not JSON", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "POST",
        path: "/webhooks/gather",
        handler: producesHandler(() => ({ type: "goodbye", text: "bye now" })),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/webhooks/gather`, { method: "POST" });
      expect(res.status).toBe(200);
      expect(res.headers.get("content-type")).toMatch(/^application\/xml/);
      const body = await res.text();
      expect(body).toBe("<Response><Say>bye now</Say></Response>");
    } finally {
      await close();
    }
  });

  it("still 500s with a JSON error body when the handler's return value fails schema validation", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "POST",
        path: "/webhooks/gather",
        // Missing `text` — fails TwimlResponseSchema before the codec ever runs.
        handler: producesHandler(() => ({ type: "goodbye" })),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/webhooks/gather`, { method: "POST" });
      expect(res.status).toBe(500);
      expect(res.headers.get("content-type")).toMatch(/^application\/json/);
    } finally {
      await close();
    }
  });
});

// --- bellowsRoutes: error handling ---

describe("bellowsRoutes error handling", () => {
  it("maps an HttpError thrown by a handler to its response", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        handler: () => { throw new NotFoundError("no such thing"); },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(404);
      expect(await res.json()).toEqual({ error: "Not Found", message: "no such thing" });
    } finally {
      await close();
    }
  });

  it("maps an HttpError rejected by an async handler to its response", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        handler: async () => { throw new ForbiddenError(); },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(403);
      expect(await res.json()).toEqual({ error: "Forbidden" });
    } finally {
      await close();
    }
  });

  it("falls back to a generic 500 for a non-HttpError", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        handler: () => { throw new Error("some internal detail"); },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(500);
      expect(await res.json()).toEqual({
        error: "Internal Server Error",
        message: "Internal server error",
      });
    } finally {
      await close();
    }
  });

  it("uses a custom errorHandler when provided", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        handler: () => { throw new NotFoundError(); },
      },
    ];
    const { url, close } = await serve(routes, {
      errorHandler: (_err, _req, res, _next) => { res.status(418).json({ error: "teapot" }); },
    });
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(418);
      expect(await res.json()).toEqual({ error: "teapot" });
    } finally {
      await close();
    }
  });

  it("skips error handling entirely when errorHandler is false", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        handler: () => { throw new NotFoundError(); },
      },
    ];
    const app = express();
    app.use(bellowsRoutes(routes, { errorHandler: false }));
    // Express's own default error handler takes over — still responds, but
    // not with the shape `errorHandler` from ./errors.js would produce.
    app.use(((err: unknown, _req, res, _next) => {
      res.status(599).json({ fallback: true });
    }) as express.ErrorRequestHandler);

    const server = http.createServer(app);
    await new Promise<void>((resolve) => server.listen(0, resolve));
    const { port } = server.address() as AddressInfo;
    try {
      const res = await fetch(`http://127.0.0.1:${port}/x`);
      expect(res.status).toBe(599);
      expect(await res.json()).toEqual({ fallback: true });
    } finally {
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  });
});

// --- bellowsRoutes: authn/authz cascade ---

describe("bellowsRoutes authn/authz", () => {
  const identify =
    (identified: boolean, user?: unknown): AuthnService["identify"] =>
    () => (identified ? { identified: true, user } : { identified: false });

  const authorize =
    (decision: "allow" | "deny" | "next"): AuthzService["authorize"] =>
    () => decision;

  it("no authn/authz declared: request passes through unchanged", async () => {
    const routes: RouteDefinition[] = [
      { method: "GET", path: "/x", handler: (_req, res) => res.json({ ok: true }) },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
    } finally {
      await close();
    }
  });

  it("authn declared, none identify: 401, handler not called", async () => {
    let handlerCalled = false;
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authn: [{ identify: identify(false) }],
        handler: (_req, res) => {
          handlerCalled = true;
          res.json({ ok: true });
        },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(401);
      expect(handlerCalled).toBe(false);
    } finally {
      await close();
    }
  });

  it("authn declared, first service identifies: handler runs, later services skipped", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authn: [
          {
            identify: () => {
              calls.push("first");
              return { identified: true, user: { id: "u1" } };
            },
          },
          {
            identify: () => {
              calls.push("second");
              return { identified: true };
            },
          },
        ],
        handler: (_req, res) => res.json({ ok: true }),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
      expect(calls).toEqual(["first"]);
    } finally {
      await close();
    }
  });

  it("authz declared, none allow: 403, handler not called", async () => {
    let handlerCalled = false;
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authz: [{ authorize: authorize("next") }],
        handler: (_req, res) => {
          handlerCalled = true;
          res.json({ ok: true });
        },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(403);
      expect(handlerCalled).toBe(false);
    } finally {
      await close();
    }
  });

  it("authz declared, a service allows: handler runs", async () => {
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authz: [{ authorize: authorize("next") }, { authorize: authorize("allow") }],
        handler: (_req, res) => res.json({ ok: true }),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
    } finally {
      await close();
    }
  });

  it("authz declared, a service denies: 403 without consulting later services", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authz: [
          {
            authorize: () => {
              calls.push("first");
              return "deny";
            },
          },
          {
            authorize: () => {
              calls.push("second");
              return "allow";
            },
          },
        ],
        handler: (_req, res) => res.json({ ok: true }),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(403);
      expect(calls).toEqual(["first"]);
    } finally {
      await close();
    }
  });

  it("authn + authz both declared: authz only runs after authn succeeds, sees the identified user", async () => {
    let seenUser: unknown;
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authn: [{ identify: identify(true, { id: "u1" }) }],
        authz: [
          {
            authorize: (_req, user) => {
              seenUser = user;
              return "allow";
            },
          },
        ],
        handler: (_req, res) => res.json({ ok: true }),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(200);
      expect(seenUser).toEqual({ id: "u1" });
    } finally {
      await close();
    }
  });

  it("authn + authz both declared, authn fails: authz never runs, 401", async () => {
    let authzCalled = false;
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authn: [{ identify: identify(false) }],
        authz: [
          {
            authorize: () => {
              authzCalled = true;
              return "allow";
            },
          },
        ],
        handler: (_req, res) => res.json({ ok: true }),
      },
    ];
    const { url, close } = await serve(routes);
    try {
      const res = await fetch(`${url}/x`);
      expect(res.status).toBe(401);
      expect(authzCalled).toBe(false);
    } finally {
      await close();
    }
  });

  it("runs authn/authz before route middleware", async () => {
    const calls: string[] = [];
    const routes: RouteDefinition[] = [
      {
        method: "GET",
        path: "/x",
        authn: [
          {
            identify: () => {
              calls.push("authn");
              return { identified: true };
            },
          },
        ],
        middleware: [
          (_req, _res, next) => {
            calls.push("middleware");
            next();
          },
        ],
        handler: (_req, res) => {
          calls.push("handler");
          res.json({ ok: true });
        },
      },
    ];
    const { url, close } = await serve(routes);
    try {
      await fetch(`${url}/x`);
      expect(calls).toEqual(["authn", "middleware", "handler"]);
    } finally {
      await close();
    }
  });
});
