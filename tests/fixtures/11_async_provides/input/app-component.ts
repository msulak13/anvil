import { Component, Singleton } from "@anvil-di/anvil";
import { ConfigModule } from "./config-module";
import { DatabaseModule } from "./database-module";
import { Config } from "./config";
import { Database } from "./database";
import { UserService } from "./user-service";

// @Singleton is required because the graph contains async @Provides
// bindings — the resolved values are cached at startup. After
// `await createAppComponent()` returns, every entry-point method runs
// synchronously and yields the already-awaited services.
@Singleton
@Component({ modules: [ConfigModule, DatabaseModule] })
export abstract class AppComponent {
  abstract config(): Config;
  abstract database(): Database;
  abstract userService(): UserService;
}
