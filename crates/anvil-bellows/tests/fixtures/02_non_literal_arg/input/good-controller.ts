import { Controller, Get } from "@anvil-di/anvil-bellows";

@Controller("/health")
export class HealthController {
  @Get("/")
  ping(req: unknown, res: unknown): void {}
}
