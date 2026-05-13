import { Controller, Get, Post } from "@msulak/anvil-bellows";

@Controller("/posts")
export class PostController {
  @Get("/")
  list(req: unknown, res: unknown): void {}

  @Post("/")
  create(req: unknown, res: unknown): void {}
}
