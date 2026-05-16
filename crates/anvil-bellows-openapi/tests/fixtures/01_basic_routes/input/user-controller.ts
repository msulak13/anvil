import {
  Controller,
  Get,
  Post,
  Delete,
  Tag,
  Security,
  Deprecated,
  Returns,
} from "@anvil-di/bellows";
import type { Body, Query, Params, Responds } from "@anvil-di/bellows";

export const CreateUserBody = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };
export const UserSchema = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };
export const UserParams = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };
export const UserQuery = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };

@Controller("/users")
@Tag("users")
@Security("bearerAuth")
export class UserController {
  @Get("/")
  list(query: Query<typeof UserQuery>): Responds<typeof UserSchema> {
    return null as any;
  }

  @Get("/:id")
  byId(params: Params<typeof UserParams>): Responds<typeof UserSchema> {
    return null as any;
  }

  @Post("/")
  create(body: Body<typeof CreateUserBody>): Responds<typeof UserSchema> {
    return null as any;
  }

  @Delete("/:id")
  @Returns(204)
  @Deprecated()
  remove(params: Params<typeof UserParams>): void {}
}
