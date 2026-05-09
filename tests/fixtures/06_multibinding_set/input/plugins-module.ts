import { Module, Provides, IntoSet } from "@msulak/anvil";
import { Plugin } from "./plugin";
import { AuthPlugin } from "./auth-plugin";
import { LoggingPlugin } from "./logging-plugin";

@Module
export class PluginsModule {
  @IntoSet
  @Provides
  static auth(): Plugin {
    return new AuthPlugin();
  }

  @IntoSet
  @Provides
  static logging(): Plugin {
    return new LoggingPlugin();
  }
}
