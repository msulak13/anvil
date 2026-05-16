import { Controller, Get } from "@anvil-di/bellows";

const BASE = "/bad";

// Non-literal @Controller argument — skips the entire controller.
@Controller(BASE)
export class BadController {
  @Get("/:id")
  byId(req: unknown, res: unknown): void {}
}
