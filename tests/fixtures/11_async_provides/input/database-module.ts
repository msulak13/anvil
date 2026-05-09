import { Module, Provides, Singleton } from "@msulak/anvil";
import { Config } from "./config";
import { Database } from "./database";

// A second async @Provides whose dep is itself async. The dagger's
// `_resolve` phase awaits in topo order: first Config, then Database
// receives the resolved Config (not a Promise<Config>).
@Module
export class DatabaseModule {
  @Singleton
  @Provides
  static async openDatabase(config: Config): Promise<Database> {
    return Promise.resolve(new Database(config.databaseUrl));
  }
}
