import { Module, Binds, Provides, IntoSet } from "@msulak/anvil";
import { Logger } from "./logger";
import { ConsoleLogger } from "./console-logger";
import { RouteRegistrar } from "./route-registrar";
import { UserController } from "./user-controller";
import { HealthController } from "./health-controller";

// One module that:
//   1. Aliases the `Logger` interface to `ConsoleLogger` via @Binds.
//   2. Contributes each controller to the `Set<RouteRegistrar>` via
//      @IntoSet @Provides — adding a new controller is one method here,
//      not a touch in `server.ts`.
@Module
export class AppModule {
  @Binds
  static bindLogger(impl: ConsoleLogger): Logger {
    return impl;
  }

  @IntoSet
  @Provides
  static provideUserRoutes(c: UserController): RouteRegistrar {
    return c;
  }

  @IntoSet
  @Provides
  static provideHealthRoutes(c: HealthController): RouteRegistrar {
    return c;
  }
}
