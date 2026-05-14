import { Module, Binds } from "@anvil-di/anvil";
import { ConsoleLogger } from "./console-logger.js";
import type { Logger } from "./logger.js";

@Module
export class AppModule {
  // Aliases the Logger interface to ConsoleLogger. Scope on the
  // ConsoleLogger binding (@Singleton) is what owns the cache; this
  // method is just a typed forwarder.
  @Binds
  static bindLogger(impl: ConsoleLogger): Logger {
    return impl;
  }
}
