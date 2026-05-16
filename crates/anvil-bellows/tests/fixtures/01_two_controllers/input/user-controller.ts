import { Controller, Delete, Get, Post } from "@anvil-di/bellows";

@Controller("/users")
export class UserController {
  @Get("/:id")
  byId(req: unknown, res: unknown): void {}

  @Post("/")
  create(req: unknown, res: unknown): void {}

  @Delete("/:id")
  remove(req: unknown, res: unknown): void {}
}
