import { Controller, Get } from "@msulak/anvil-bellows";

@Controller("/health")
export class HealthController {
  @Get("/")
  ping(req: unknown, res: unknown): void {}
}
