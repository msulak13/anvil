import { Component, Singleton } from "@msulak/anvil";
import { AppModule } from "./app-module";
import { Logger } from "./logger";
import { RouteRegistrar } from "./route-registrar";

@Singleton
@Component({ modules: [AppModule] })
export abstract class AppComponent {
  // Exposed for app-level concerns (startup logging, error handlers, …).
  abstract logger(): Logger;

  // The plugin-discovered registrar set. `server.ts` iterates over this
  // and binds each registrar's routes onto the Express app.
  abstract routes(): Set<RouteRegistrar>;
}
