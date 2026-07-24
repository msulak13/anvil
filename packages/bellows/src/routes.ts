import { Router, type Request, type RequestHandler, type Response } from "express";
import type { AuthnService, AuthzService } from "./authz.js";

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  /**
   * Ordered authentication cascade from `@Authn(...)` decorators (class-level
   * first, then method-level). The first service whose `identify()` returns
   * `identified: true` wins and its `user` becomes `res.locals.user`. When
   * this is non-empty and none identify the requester, the route fails
   * closed with 401 — undeclared (empty/absent) means no authn check runs.
   */
  authn?: AuthnService[];
  /**
   * Ordered authorization cascade from `@Authz(...)` decorators (class-level
   * first, then method-level), run only after authn succeeds (or when no
   * `authn` is declared). The first non-`"next"` decision wins. When this is
   * non-empty and every service returns `"next"` (or one returns `"deny"`),
   * the route fails closed with 403 — undeclared means no authz check runs.
   */
  authz?: AuthzService[];
  /**
   * Ordered middleware run after authn/authz and before `handler`, from
   * `@Middleware(...)` decorators (class-level first, then method-level). A
   * middleware that doesn't call `next()` short-circuits the request —
   * standard Express semantics.
   */
  middleware?: RequestHandler[];
  handler: (req: Request, res: Response) => void | Promise<void>;
}

/**
 * Build the combined authn → authz `RequestHandler` for a route, or
 * `undefined` when neither is declared (so undeclared routes are unaffected).
 */
function buildAuthHandler(
  authn: AuthnService[] = [],
  authz: AuthzService[] = [],
): RequestHandler | undefined {
  if (authn.length === 0 && authz.length === 0) {
    return undefined;
  }
  return (req, res, next) => {
    void (async () => {
      let user: unknown;
      if (authn.length > 0) {
        let identified = false;
        for (const service of authn) {
          const result = await service.identify(req);
          if (result.identified) {
            identified = true;
            user = result.user;
            break;
          }
        }
        if (!identified) {
          res.status(401).json({ error: "unauthorized" });
          return;
        }
      }
      if (authz.length > 0) {
        let allowed = false;
        for (const service of authz) {
          const decision = await service.authorize(req, user);
          if (decision === "allow") {
            allowed = true;
            break;
          }
          if (decision === "deny") {
            break;
          }
        }
        if (!allowed) {
          res.status(403).json({ error: "forbidden" });
          return;
        }
      }
      res.locals.user = user;
      next();
    })();
  };
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
    const authHandler = buildAuthHandler(route.authn, route.authz);
    const handlers: RequestHandler[] = [
      ...(authHandler ? [authHandler] : []),
      ...(route.middleware ?? []),
      route.handler,
    ];
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
