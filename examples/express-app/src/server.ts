// Express entry point.
//
// The dagger is constructed once at startup. Every request goes through
// a tiny middleware that calls `dagger.requestComponent(req, res)`,
// which yields a fresh per-request graph for THIS request only.
// Handlers reach into that per-request dagger to get controllers
// pre-wired with the right `Request`, `Response`, `RequestContext`,
// and inherited app-scoped services.
import express from "express";
import type { NextFunction, Request, Response } from "express";
import { createAppComponent } from "./app-component.tsdi.js";
import type { RequestComponent } from "./request-component.js";

const app = express();
app.use(express.json());

const dagger = createAppComponent();
const log = dagger.logger();

// Stash the per-request dagger on `res.locals` so route handlers can
// reach it without taking it as a parameter. We type the access point
// explicitly rather than module-augmenting express's types — keeps the
// example free of `tsconfig.json` `paths` / module-augmentation quirks.
type ResWithDi = Response & { locals: { di: RequestComponent } };

app.use((req: Request, res: Response, next: NextFunction): void => {
  (res as ResWithDi).locals.di = dagger.requestComponent(req, res);
  next();
});

// Each handler is a thin wrapper around a controller method. The
// controller already has `req`, `res`, the request-scoped
// RequestContext, and inherited Logger / UserRepository wired in.
app.get("/users", (_req, res) => {
  (res as ResWithDi).locals.di.userController().list();
});

app.get("/users/:id", (req, res) => {
  (res as ResWithDi).locals.di.userController().byId(req.params.id);
});

app.post("/users", (req, res) => {
  const body = req.body as { name?: unknown; email?: unknown };
  (res as ResWithDi).locals.di.userController().create(body.name, body.email);
});

app.get("/whoami", (_req, res) => {
  (res as ResWithDi).locals.di.userController().whoami();
});

const port = Number(process.env["PORT"] ?? 3000);
app.listen(port, () => {
  log.info(`tsdi express-app listening on :${port}`);
});
