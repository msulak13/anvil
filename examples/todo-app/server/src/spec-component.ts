import { Component, Singleton } from "@msulak/anvil";
import type { RouteDefinition } from "@msulak/anvil-bellows";
import { OpenApiModule } from "./schema-route.module.anvil.js";

@Singleton
@Component({ modules: [OpenApiModule] })
export abstract class SpecComponent {
  abstract routeDefinitions(): Set<RouteDefinition>;
}
