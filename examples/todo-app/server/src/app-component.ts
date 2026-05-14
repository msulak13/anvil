import { Component, Singleton } from "@anvil-di/anvil";
import type { RouteDefinition } from "@anvil-di/anvil-bellows";
import { RoutesModule } from "./routes.module.anvil.js";
import { TodoModule } from "./todo-module.js";
import { OpenApiModule } from "./schema-route.module.anvil.js";

@Singleton
@Component({ modules: [TodoModule, RoutesModule, OpenApiModule] })
export abstract class AppComponent {
  abstract routeDefinitions(): Set<RouteDefinition>;
}
