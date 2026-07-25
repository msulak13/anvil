import { Controller, Post } from "@anvil-di/bellows";
import type { FormBody, Headers, RawBody } from "@anvil-di/bellows";

type SafeParseResult<T> = { success: true; data: T } | { success: false; error: unknown };

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
