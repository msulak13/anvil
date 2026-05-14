import { Component, Singleton } from "@anvil-di/anvil";
import type { RouteDefinition } from "@anvil-di/anvil-bellows";
import { OpenApiModule } from "./schema-route.module.anvil.js";

@Singleton
@Component({ modules: [OpenApiModule] })
export abstract class SpecComponent {
  abstract routeDefinitions(): Set<RouteDefinition>;
}
