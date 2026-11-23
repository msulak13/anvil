import { Module, Provides, Singleton } from "@anvil-di/anvil";
import { Config } from "./config";
import { Database } from "./database";

@Module
export class DatabaseModule {
  @Singleton
  @Provides
  static async open(config: Config): Promise<Database> {
    return new Database(config.databaseUrl);
  }
}
