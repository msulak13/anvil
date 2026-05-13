import type { IRouter, Request, Response } from "express";

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  handler: (req: Request, res: Response) => void | Promise<void>;
}

/** Register every route in `routes` on `router` (or an Express app). */
export function applyRoutes(
  router: IRouter,
  routes: Iterable<RouteDefinition>,
): void {
  for (const route of routes) {
    switch (route.method) {
      case "GET":    router.get(route.path, route.handler);    break;
      case "POST":   router.post(route.path, route.handler);   break;
      case "PUT":    router.put(route.path, route.handler);    break;
      case "DELETE": router.delete(route.path, route.handler); break;
      case "PATCH":  router.patch(route.path, route.handler);  break;
    }
  }
}
