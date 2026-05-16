import { Controller, Get } from "@anvil-di/bellows";

@Controller("/health")
export class HealthController {
  @Get("/")
  ping(req: unknown, res: unknown): void {}
}
