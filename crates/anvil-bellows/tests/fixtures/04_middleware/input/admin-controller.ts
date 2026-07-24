import { Controller, Get, Middleware } from "@anvil-di/bellows";
import { requireAdmin, requireAuth } from "./auth-middleware";

export function auditLog(req: unknown, res: unknown, next: () => void): void {
  next();
}

@Controller("/admin")
@Middleware(requireAuth)
export class AdminController {
  @Get("/dashboard")
  dashboard(req: unknown, res: unknown): void {}

  @Get("/stats")
  @Middleware(requireAdmin, auditLog)
  stats(req: unknown, res: unknown): void {}
}
