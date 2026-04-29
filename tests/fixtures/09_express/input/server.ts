// Real entry point — sits OUTSIDE the @Component import closure (no
// dagger binding imports this file), so the parser never tries to
// resolve `express` and Express stays out of the DI graph entirely.
//
// The dagger gives us a `Set<RouteRegistrar>`; we walk it once at
// startup and translate each `register` call into the matching
// `app.get` / `app.post` / etc. on the Express instance.
import express, { Express, Request, RequestHandler, Response } from "express";
import { createAppComponent } from "./app-component.tsdi";
import { Method } from "./route-registrar";

const dagger = createAppComponent();
const log = dagger.logger();
const app: Express = express();
app.use(express.json());

const expressMethod: Record<Method, keyof Pick<Express, "get" | "post" | "put" | "patch" | "delete">> = {
  GET: "get",
  POST: "post",
  PUT: "put",
  PATCH: "patch",
  DELETE: "delete",
};

for (const registrar of dagger.routes()) {
  registrar.register((method, path, handler) => {
    const wrapped: RequestHandler = (req: Request, res: Response) => {
      handler(req, res);
    };
    app[expressMethod[method]](path, wrapped);
    log.info(`mounted ${method} ${path}`);
  });
}

const port = Number(process.env["PORT"] ?? 3000);
app.listen(port, () => log.info(`listening on :${port}`));
