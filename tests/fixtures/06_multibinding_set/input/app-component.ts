import { Component } from "@msulak/anvil";
import { PluginsModule } from "./plugins-module";
import { Plugin } from "./plugin";

@Component({ modules: [PluginsModule] })
export abstract class AppComponent {
  abstract plugins(): Set<Plugin>;
}
