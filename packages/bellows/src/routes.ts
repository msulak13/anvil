import express, {
  Router,
  type ErrorRequestHandler,
  type Request,
  type RequestHandler,
  type Response,
} from "express";
import type { AuthnService, AuthzService } from "./authz.js";
import { errorHandler } from "./errors.js";

declare global {
  namespace Express {
    interface Request {
      /** Raw request body bytes, captured by the body parser `bellowsRoutes`
       *  mounts for any route declaring a `RawBody` param. */
      rawBody?: Buffer;
    }
  }
}

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  /**
   * Which body parser to mount for this route, derived by codegen from its
   * `Body<S>`/`FormBody<S>`/`RawBody`/two-arg `Body<S, C>` params: `"json"`
   * or `"urlencoded"` populate `req.body` via Express's built-in parsers,
   * `"raw"` leaves `req.body` as the raw `Buffer` (no parsed param declared
   * alongside `RawBody`), and the `{ kind: "codec", ... }` form — from
   * `Body<S, C>` — mounts `express.raw()` scoped to `contentType` and
   * replaces `req.body` with `decode()`'s result before the handler's
   * `S.safeParse(req.body)` runs. All variants also capture the exact wire
   * bytes into `req.rawBody`. Absent when the route declares none of those
   * params — unaffected, as before.
   */
  bodyParser?:
    | "json"
    | "urlencoded"
    | "raw"
    | {
        kind: "codec";
        contentType: string;
        decode: (raw: Buffer) => unknown;
      };
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

/**
 * Build the body-parser `RequestHandler` for a route's `bodyParser` kind. All
 * variants share a `verify` callback that stashes the exact wire bytes on
 * `req.rawBody`, so `RawBody` params work whether or not the route also
 * parses `req.body` via `Body<S>`/`FormBody<S>`/the two-arg `Body<S, C>` form.
 */
function bodyParserMiddleware(kind: NonNullable<RouteDefinition["bodyParser"]>): RequestHandler {
  const verify = (req: Request, _res: Response, buf: Buffer): void => {
    req.rawBody = buf;
  };
  if (typeof kind === "object") {
    const { contentType, decode } = kind;
    const rawParser = express.raw({ type: contentType, verify });
    return (req, res, next) => {
      rawParser(req, res, (err: unknown) => {
        if (err) {
          next(err);
          return;
        }
        // Content-Type didn't match `contentType` — express.raw() left
        // req.body untouched (no other parser ran), so there's nothing to
        // decode; the route's S.safeParse(req.body) will reject it as
        // expected.
        if (!Buffer.isBuffer(req.body)) {
          next();
          return;
        }
        try {
          req.body = decode(req.body);
        } catch (decodeError) {
          next(decodeError);
          return;
        }
        next();
      });
    };
  }
  switch (kind) {
    case "json":
      return express.json({ verify });
    case "urlencoded":
      return express.urlencoded({ extended: true, verify });
    case "raw":
      return express.raw({ type: "*/*", verify });
  }
}

/** Request-lifecycle hooks that apply to every route registered via `bellowsRoutes`. */
export interface BellowsHooks {
  /** Runs before any route's middleware/handler, for every request. */
  onRequest?: RequestHandler;
  /** Runs after the response has been sent, for every request. */
  onResponse?: (req: Request, res: Response) => void;
  /**
   * Error-handling middleware registered after all routes, so an `HttpError`
   * thrown (or passed to `next`) by any route's auth handler, middleware, or
   * handler is converted to a response — Express 5 forwards rejected
   * promises from async handlers automatically. Defaults to `errorHandler`
   * from `./errors.js`. Pass `false` to opt out, e.g. when a shared instance
   * is mounted once on the parent app instead of per-router.
   */
  errorHandler?: ErrorRequestHandler | false;
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
      ...(route.bodyParser ? [bodyParserMiddleware(route.bodyParser)] : []),
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

  if (hooks.errorHandler !== false) {
    router.use(hooks.errorHandler ?? errorHandler);
  }

  return router;
}
