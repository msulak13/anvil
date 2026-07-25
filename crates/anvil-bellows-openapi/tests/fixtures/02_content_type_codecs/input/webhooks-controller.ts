import { Controller, Post } from "@anvil-di/bellows";
import type { Body, Produces, RequestCodec, ResponseCodec } from "@anvil-di/bellows";

export const GatherCallbackSchema = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };
export const TwimlResponseSchema = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };
export const GreetingBody = { safeParse: (x: unknown) => ({ success: true as const, data: x }) };

export const twimlRequestCodec: RequestCodec<Body<typeof GatherCallbackSchema>> = {
  contentType: "application/xml",
  decode: (raw) => ({ digits: raw.toString() }),
};

export const twimlResponseCodec: ResponseCodec<any> = {
  contentType: "application/xml",
  encode: (value) => `<Response>${JSON.stringify(value)}</Response>`,
};

@Controller("/webhooks")
export class WebhooksController {
  // Plain single-arg Body<S> — still documented as application/json.
  @Post("/greeting")
  greeting(body: Body<typeof GreetingBody>): void {}

  // Two-arg Body<S, C> — content type resolved from the codec's literal.
  @Post("/gather")
  gather(body: Body<typeof GatherCallbackSchema, typeof twimlRequestCodec>): void {}

  // Produces<S, C> — response content type resolved from the codec's literal.
  @Post("/say")
  async say(): Promise<Produces<typeof TwimlResponseSchema, typeof twimlResponseCodec>> {
    return {} as any;
  }
}
