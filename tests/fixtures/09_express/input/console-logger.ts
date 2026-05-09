import { Inject, Singleton } from "@msulak/anvil";
import { Logger } from "./logger";

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
