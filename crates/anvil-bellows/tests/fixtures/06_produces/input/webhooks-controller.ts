import { Controller, Post } from "@anvil-di/bellows";
import type { Body, Produces, Responds, ResponseCodec } from "@anvil-di/bellows";

// `error` mirrors zod's `ZodError` shape closely enough for typecheck purposes: a
// `.message` string (zod's `ZodError.message` is a JSON-stringified issue list).
type SafeParseResult<T> = { success: true; data: T } | { success: false; error: { message: string } };

function safeParse<T>(input: unknown): SafeParseResult<T> {
  return { success: true, data: input as T };
}

export const GatherWebhookBody = { safeParse };
export const TwimlResponseSchema = { safeParse };

export const twimlCodec: ResponseCodec<Responds<typeof TwimlResponseSchema>> = {
  contentType: "application/xml",
  encode: (value) => `<Response><Say>${JSON.stringify(value)}</Say></Response>`,
};

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  async gather(
    body: Body<typeof GatherWebhookBody>,
  ): Promise<Produces<typeof TwimlResponseSchema, typeof twimlCodec>> {
    return {} as any;
  }
}
