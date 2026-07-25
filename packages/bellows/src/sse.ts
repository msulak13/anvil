import type { Request, Response } from "express";

/** Default interval for keep-alive comment pings, in milliseconds. */
const DEFAULT_KEEPALIVE_MS = 15_000;

export interface SseSendOptions {
  /** `event:` field — the client-side listener name (defaults to `"message"`). */
  event?: string;
  /** `id:` field — sets `EventSource.lastEventId` for reconnects. */
  id?: string;
  /** `retry:` field — reconnection delay in ms the client should use. */
  retry?: number;
}

/**
 * An `AbortSignal` that fires when the client disconnects (`req`'s underlying
 * socket closes). Shared by `SseStream` and available standalone for handlers
 * that inject `AbortSignal` without an `SseStream` — e.g. a non-SSE chunked
 * streaming handler that only needs disconnect-driven cleanup.
 */
export function disconnectSignal(req: Request): AbortSignal {
  const controller = new AbortController();
  req.on("close", () => controller.abort());
  return controller.signal;
}

/**
 * Wraps `res` for a Server-Sent Events route (`@Sse`). Handles the
 * `text/event-stream` handshake, event framing, keep-alive pings, and
 * exposes `signal` so the handler can clean up subscriptions when the
 * client disconnects.
 *
 * Construction does not write anything — call `open()` once the handler is
 * ready to commit to a streaming response (e.g. after validating params).
 */
export class SseStream {
  readonly signal: AbortSignal;
  private closed = false;
  private keepAliveTimer: ReturnType<typeof setInterval> | undefined;

  constructor(
    private readonly res: Response,
    signal: AbortSignal,
  ) {
    this.signal = signal;
    signal.addEventListener("abort", () => this.teardown());
  }

  /**
   * Sets SSE headers and flushes them immediately so the client sees an open
   * connection before the first event. Starts the keep-alive ping interval
   * unless `keepAliveMs` is `false`.
   */
  open(keepAliveMs: number | false = DEFAULT_KEEPALIVE_MS): this {
    if (this.res.headersSent) return this;
    this.res.status(200);
    this.res.setHeader("Content-Type", "text/event-stream; charset=utf-8");
    this.res.setHeader("Cache-Control", "no-cache, no-transform");
    this.res.setHeader("Connection", "keep-alive");
    // Disables response buffering on nginx-fronted deployments, which would
    // otherwise hold the stream open with no bytes reaching the client.
    this.res.setHeader("X-Accel-Buffering", "no");
    this.res.flushHeaders();
    if (keepAliveMs !== false) {
      this.keepAliveTimer = setInterval(() => this.comment("ping"), keepAliveMs);
      this.keepAliveTimer.unref?.();
    }
    return this;
  }

  /** Writes one SSE event frame. Objects are JSON-encoded; strings are sent as-is. */
  send(data: unknown, opts: SseSendOptions = {}): void {
    if (this.closed) return;
    let frame = "";
    if (opts.id !== undefined) frame += `id: ${opts.id}\n`;
    if (opts.event !== undefined) frame += `event: ${opts.event}\n`;
    if (opts.retry !== undefined) frame += `retry: ${opts.retry}\n`;
    const payload = typeof data === "string" ? data : JSON.stringify(data);
    for (const line of payload.split("\n")) frame += `data: ${line}\n`;
    this.res.write(`${frame}\n`);
  }

  /** Writes an SSE comment line — invisible to `EventSource` listeners, used for keep-alive. */
  comment(text = ""): void {
    if (this.closed) return;
    this.res.write(`: ${text}\n\n`);
  }

  /** Ends the response. Safe to call more than once, or after client disconnect. */
  close(): void {
    if (this.closed) return;
    this.teardown();
    this.res.end();
  }

  private teardown(): void {
    if (this.closed) return;
    this.closed = true;
    if (this.keepAliveTimer) clearInterval(this.keepAliveTimer);
  }
}
