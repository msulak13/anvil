import { Module, Provides, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { Env } from "./env";

@Module
export class DatabaseModule {
  @Singleton
  @Provides
  static async open(env: Env): Promise<Database> {
    return new Database(env.databaseUrl);
  }
}
