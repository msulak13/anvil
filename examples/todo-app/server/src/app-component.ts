import { Component, Singleton } from "@msulak/anvil";
import type { RouteDefinition } from "@msulak/anvil-bellows";
import { RoutesModule } from "./routes.module.anvil.js";
import { TodoModule } from "./todo-module.js";

@Singleton
@Component({ modules: [TodoModule, RoutesModule] })
export abstract class AppComponent {
  abstract routeDefinitions(): Set<RouteDefinition>;
}
