import { Component, Singleton } from "@anvil-di/anvil";
import { AppModule } from "./app-module";
import { Logger } from "./logger";
import { UserService } from "./user-service";

// @Singleton on the component is required because the graph contains
// @Singleton bindings (ConsoleLogger, UserRepository). The codegen
// emits a `DaggerAppComponent` with cache fields for each.
//
// Entry-point methods are how the rest of the application reaches into
// the graph — `server.ts` calls `createAppComponent().userService()`.
@Singleton
@Component({ modules: [AppModule] })
export abstract class AppComponent {
  abstract logger(): Logger;
  abstract userService(): UserService;
}
