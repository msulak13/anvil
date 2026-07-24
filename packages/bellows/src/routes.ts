import { Router, type Request, type RequestHandler, type Response } from "express";

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  /**
   * Ordered middleware run before `handler`, from `@Middleware(...)`
   * decorators (class-level first, then method-level). A middleware that
   * doesn't call `next()` short-circuits the request — standard Express
   * semantics.
   */
  middleware?: RequestHandler[];
  handler: (req: Request, res: Response) => void | Promise<void>;
}

/** Request-lifecycle hooks that apply to every route registered via `bellowsRoutes`. */
export interface BellowsHooks {
  /** Runs before any route's middleware/handler, for every request. */
  onRequest?: RequestHandler;
  /** Runs after the response has been sent, for every request. */
  onResponse?: (req: Request, res: Response) => void;
}

/** Returns an Express Router with all routes from `routes` registered on it. */
export function bellowsRoutes(routes: Iterable<RouteDefinition>, hooks: BellowsHooks = {}): Router {
  const router = Router();

  if (hooks.onRequest) {
    router.use(hooks.onRequest);
  }
  if (hooks.onResponse) {
    const onResponse = hooks.onResponse;
    router.use((req, res, next) => {
      res.on("finish", () => onResponse(req, res));
      next();
    });
  }

  for (const route of routes) {
    const handlers: RequestHandler[] = [...(route.middleware ?? []), route.handler];
    switch (route.method) {
      case "GET":    router.get(route.path, ...handlers);    break;
      case "POST":   router.post(route.path, ...handlers);   break;
      case "PUT":    router.put(route.path, ...handlers);    break;
      case "DELETE": router.delete(route.path, ...handlers); break;
      case "PATCH":  router.patch(route.path, ...handlers);  break;
    }
  }
  return router;
}
