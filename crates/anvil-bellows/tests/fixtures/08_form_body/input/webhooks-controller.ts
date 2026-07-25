import { Controller, Post } from "@anvil-di/bellows";
import type { FormBody, Headers, RawBody } from "@anvil-di/bellows";

// `error` mirrors zod's `ZodError` shape closely enough for typecheck purposes: a
// `.message` string (zod's `ZodError.message` is a JSON-stringified issue list).
type SafeParseResult<T> = { success: true; data: T } | { success: false; error: { message: string } };

function safeParse<T>(input: unknown): SafeParseResult<T> {
  return { success: true, data: input as T };
}

export const GatherBody = { safeParse };
export const SignatureHeaders = { safeParse };

@Controller("/webhooks")
export class WebhooksController {
  @Post("/gather")
  gather(
    body: FormBody<typeof GatherBody>,
    headers: Headers<typeof SignatureHeaders>,
    raw: RawBody,
  ): void {}
}
