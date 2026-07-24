import type { AuthnUser } from "@anvil-di/bellows";
import { Authn, Authz, Controller, Get } from "@anvil-di/bellows";
import { RoleAuthz, SessionAuthn, type User } from "./auth-services";

@Controller("/admin")
@Authn(SessionAuthn)
export class AdminController {
  @Get("/dashboard")
  dashboard(req: unknown, res: unknown): void {}

  @Get("/stats")
  @Authz(RoleAuthz)
  stats(user: AuthnUser<User>): void {}
}
