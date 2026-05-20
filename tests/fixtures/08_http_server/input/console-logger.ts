import { Inject, Singleton } from "@anvil-di/anvil";
import { Logger } from "./logger";

// @Singleton + @Inject: one ConsoleLogger per AppComponent, lazily
// constructed and cached via the dagger's `_consoleLogger` field.
@Inject
@Singleton
export class ConsoleLogger implements Logger {
  info(message: string): void {
    console.log(`[info] ${message}`);
  }
  warn(message: string): void {
    console.warn(`[warn] ${message}`);
  }
}
