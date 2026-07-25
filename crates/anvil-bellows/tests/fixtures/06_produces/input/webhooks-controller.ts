import { Controller, Post } from "@anvil-di/bellows";
import type { Body, Produces, Responds, ResponseCodec } from "@anvil-di/bellows";

type SafeParseResult<T> = { success: true; data: T } | { success: false; error: unknown };

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
