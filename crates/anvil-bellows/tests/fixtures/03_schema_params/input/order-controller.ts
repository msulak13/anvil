import { Controller, Post, Get, Delete } from "@anvil-di/bellows";
import type { Body, Query, Params, Responds } from "@anvil-di/bellows";

// Minimal stand-ins for express.Request / express.Response in the test environment.
type Request = any;
type Response = any;

// `error` mirrors zod's `ZodError` shape closely enough for typecheck purposes: a
// `.message` string (zod's `ZodError.message` is a JSON-stringified issue list).
type SafeParseResult<T> = { success: true; data: T } | { success: false; error: { message: string } };

function safeParse<T>(input: unknown): SafeParseResult<T> {
  return { success: true, data: input as T };
}

export const CreateOrderBody = { safeParse };
export const OrderFilterQuery = { safeParse };
export const OrderParams = { safeParse };
export const OrderSchema = { safeParse };

@Controller("/orders")
export class OrderController {
  @Post("/")
  create(body: Body<typeof CreateOrderBody>): Responds<typeof OrderSchema> {
    return {} as any;
  }

  @Get("/")
  list(query: Query<typeof OrderFilterQuery>): Responds<typeof OrderSchema> {
    return {} as any;
  }

  @Get("/:id")
  byId(params: Params<typeof OrderParams>, req: Request): Responds<typeof OrderSchema> {
    return {} as any;
  }

  @Delete("/:id")
  remove(params: Params<typeof OrderParams>, res: Response): void {}
}
