import type { AuthnUser } from "@anvil-di/bellows";
import { Authn, Authz, Controller, Get, Middleware, Returns, Tag } from "@anvil-di/bellows";
import { auditLog, requireAuth } from "./auth-middleware";
import { RoleAuthz, SessionAuthn, type User } from "./auth-services";

@Controller("/admin")
@Authn(SessionAuthn)
@Middleware(requireAuth)
@Tag("admin")
export class AdminController {
  @Get("/dashboard")
  dashboard(req: unknown, res: unknown): void {}

  @Get("/stats")
  @Authz(RoleAuthz)
  @Middleware(auditLog)
  @Returns(403)
  stats(user: AuthnUser<User>): void {}
}
