import { Inject, Singleton } from "@anvil-di/anvil";

@Inject
@Singleton
export class Config {
  readonly databaseUrl = "pg://db";
}
