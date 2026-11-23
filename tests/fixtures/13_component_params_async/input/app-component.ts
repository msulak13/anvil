import { Component, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { DatabaseModule } from "./database-module";
import { Env } from "./env";
import { UserService } from "./user-service";

@Singleton
@Component({ modules: [DatabaseModule] })
export abstract class AppComponent {
  constructor(env: Env) {}

  abstract database(): Database;
  abstract userService(): UserService;
}
