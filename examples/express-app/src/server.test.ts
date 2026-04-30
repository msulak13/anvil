import { describe, expect, it } from "vitest";
import express from "express";
import type { NextFunction, Request, Response } from "express";
import request from "supertest";
import { createAppComponent } from "./app-component.tsdi.js";
import type { RequestComponent } from "./request-component.js";

type ResWithDi = Response & { locals: { di: RequestComponent } };

function buildApp(): express.Express {
  const app = express();
  app.use(express.json());
  const dagger = createAppComponent();

  app.use((_req: Request, res: Response, next: NextFunction): void => {
    (res as ResWithDi).locals.di = dagger.requestComponent(_req, res);
    next();
  });
  app.get("/users", (_req, res) =>
    (res as ResWithDi).locals.di.userController().list(),
  );
  app.get("/users/:id", (req, res) =>
    (res as ResWithDi).locals.di.userController().byId(req.params.id),
  );
  app.post("/users", (req, res) => {
    const body = req.body as { name?: unknown; email?: unknown };
    (res as ResWithDi).locals.di.userController().create(body.name, body.email);
  });
  app.get("/whoami", (_req, res) =>
    (res as ResWithDi).locals.di.userController().whoami(),
  );
  app.get("/echo-context", (_req, res) =>
    res.json((res as ResWithDi).locals.di.context()),
  );
  return app;
}

describe("tsdi express-app", () => {
  it("lists seeded users (app-scoped repository shared across requests)", async () => {
    const app = buildApp();
    const res = await request(app).get("/users").expect(200);
    expect(res.body).toEqual([
      { id: 1, name: "Alice", email: "alice@example.com" },
      { id: 2, name: "Bob", email: "bob@example.com" },
    ]);
  });

  it("create + list shows the new user (proves repo is one shared @Singleton)", async () => {
    const app = buildApp();
    const created = await request(app)
      .post("/users")
      .send({ name: "Carol", email: "carol@example.com" })
      .expect(201);
    expect(created.body).toMatchObject({
      id: 3,
      name: "Carol",
      email: "carol@example.com",
    });
    const list = await request(app).get("/users").expect(200);
    expect(list.body).toHaveLength(3);
    expect(list.body[2]).toMatchObject({ name: "Carol" });
  });

  it("404s on unknown id with a JSON error body", async () => {
    const app = buildApp();
    const res = await request(app).get("/users/9999").expect(404);
    expect(res.body).toEqual({ error: "user not found" });
  });

  it("threads the request-scoped RequestContext fresh per request", async () => {
    const app = buildApp();
    const a = await request(app)
      .get("/echo-context")
      .set("x-request-id", "req-a")
      .set("x-user-id", "1")
      .expect(200);
    const b = await request(app)
      .get("/echo-context")
      .set("x-request-id", "req-b")
      .set("x-user-id", "2")
      .expect(200);
    expect(a.body.requestId).toBe("req-a");
    expect(a.body.userId).toBe(1);
    expect(b.body.requestId).toBe("req-b");
    expect(b.body.userId).toBe(2);
    // Each request got its own RequestContext object — proves the
    // factory-param subcomponent is fresh per call.
    expect(a.body).not.toEqual(b.body);
  });

  it("whoami uses the request-scoped userId to look up an app-scoped user", async () => {
    const app = buildApp();
    const res = await request(app)
      .get("/whoami")
      .set("x-user-id", "1")
      .set("x-request-id", "test-1")
      .expect(200);
    expect(res.body.user).toMatchObject({ id: 1, name: "Alice" });
    expect(res.body.requestId).toBe("test-1");
  });

  it("whoami 401s when the request lacks the header", async () => {
    const app = buildApp();
    const res = await request(app).get("/whoami").expect(401);
    expect(res.body).toEqual({ error: "missing x-user-id header" });
  });
});
