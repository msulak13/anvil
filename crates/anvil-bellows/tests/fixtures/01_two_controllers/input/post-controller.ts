import { Controller, Get, Post } from "@anvil-di/bellows";

@Controller("/posts")
export class PostController {
  @Get("/")
  list(req: unknown, res: unknown): void {}

  @Post("/")
  create(req: unknown, res: unknown): void {}
}
