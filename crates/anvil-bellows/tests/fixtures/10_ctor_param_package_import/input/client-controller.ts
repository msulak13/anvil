import { Controller, Get } from "@anvil-di/bellows";
import type { AcmeClient } from "acme-sdk";

@Controller("/client")
export class ClientController {
  constructor(private readonly client: AcmeClient) {}

  @Get("/")
  ping(req: unknown, res: unknown): void {}
}
