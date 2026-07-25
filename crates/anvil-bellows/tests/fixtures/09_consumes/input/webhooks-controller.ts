import { Controller, Post } from "@anvil-di/bellows";
import type { Body, Consumes, RequestCodec } from "@anvil-di/bellows";

// `error` mirrors zod's `ZodError` shape closely enough for typecheck purposes: a
// `.message` string (zod's `ZodError.message` is a JSON-stringified issue list).
type SafeParseResult<T> = { success: true; data: T } | { success: false; error: { message: string } };

function safeParse<T>(input: unknown): SafeParseResult<T> {
  return { success: true, data: input as T };
}

export const GatherCallbackSchema = { safeParse };

export const twimlRequestCodec: RequestCodec<Body<typeof GatherCallbackSchema>> = {
  contentType: "application/xml",
  decode: (raw) => ({ digits: raw.toString() }),
};

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(body: Consumes<typeof GatherCallbackSchema, typeof twimlRequestCodec>): void {}
}
