import { Module, Binds } from "tsdi";
import { Logger } from "./logger";
import { ConsoleLogger } from "./console-logger";

// Aliases the Logger interface to ConsoleLogger. The Binds factory
// itself is Unscoped — caching lives on ConsoleLogger's @Singleton
// binding, so every getLogger() call returns the same cached instance.
@Module
export class AppModule {
  @Binds
  static bindLogger(impl: ConsoleLogger): Logger {
    return impl;
  }
}
