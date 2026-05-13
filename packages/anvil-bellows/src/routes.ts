import type { Request, Response } from "express";

export interface RouteDefinition {
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
  path: string;
  handler: (req: Request, res: Response) => void | Promise<void>;
}
