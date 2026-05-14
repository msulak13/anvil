import { Inject, Singleton } from "@anvil-di/anvil";
import type { Logger } from "./logger.js";

@Inject
@Singleton
export class ConsoleLogger implements Logger {
  info(message: string, meta?: Record<string, unknown>): void {
    this.write("info", message, meta);
  }
  warn(message: string, meta?: Record<string, unknown>): void {
    this.write("warn", message, meta);
  }
  error(message: string, meta?: Record<string, unknown>): void {
    this.write("error", message, meta);
  }

  private write(level: string, message: string, meta?: Record<string, unknown>): void {
    const line = meta === undefined ? message : `${message} ${JSON.stringify(meta)}`;
    // eslint-disable-next-line no-console
    console.log(`[${level}] ${line}`);
  }
}
