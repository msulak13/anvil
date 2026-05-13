import { Router, type Request, type Response } from "express";

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  handler: (req: Request, res: Response) => void | Promise<void>;
}

/** Returns an Express Router with all routes from `routes` registered on it. */
export function bellowsRoutes(routes: Iterable<RouteDefinition>): Router {
  const router = Router();
  for (const route of routes) {
    switch (route.method) {
      case "GET":    router.get(route.path, route.handler);    break;
      case "POST":   router.post(route.path, route.handler);   break;
      case "PUT":    router.put(route.path, route.handler);    break;
      case "DELETE": router.delete(route.path, route.handler); break;
      case "PATCH":  router.patch(route.path, route.handler);  break;
    }
  }
  return router;
}
