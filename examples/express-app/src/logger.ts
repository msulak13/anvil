// App-scoped logger. Lives on the parent component so every request
// shares the same instance (and any per-request log context is folded
// in by callers, not by the logger itself).

export interface Logger {
  info(message: string, meta?: Record<string, unknown>): void;
  warn(message: string, meta?: Record<string, unknown>): void;
  error(message: string, meta?: Record<string, unknown>): void;
}
