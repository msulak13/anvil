import type { AuthnUser } from "@anvil-di/bellows";
import { Controller, Get } from "@anvil-di/bellows";
import type { User } from "./auth-services";
import { AdminArea, Audited } from "./policies";

@Controller("/admin")
@AdminArea
export class AdminController {
  @Get("/dashboard")
  dashboard(req: unknown, res: unknown): void {}

  @Get("/stats")
  @Audited
  stats(user: AuthnUser<User>): void {}
}
