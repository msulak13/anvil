import { Controller, Sse, Get } from "@anvil-di/bellows";
import type { Params, SseStream } from "@anvil-di/bellows";

// Minimal stand-ins for express.Request / express.Response in the test environment.
type Request = any;
type Response = any;

type SafeParseResult<T> = { success: true; data: T } | { success: false; error: { message: string } };

function safeParse<T>(input: unknown): SafeParseResult<T> {
  return { success: true, data: input as T };
}

export const JobParams = { safeParse };

@Controller("/events")
export class EventsController {
  @Sse("/progress/:jobId")
  async progress(params: Params<typeof JobParams>, stream: SseStream, signal: AbortSignal): Promise<void> {}

  @Get("/ping")
  ping(res: Response): void {}
}
