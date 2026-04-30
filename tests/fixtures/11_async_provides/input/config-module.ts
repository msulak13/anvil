import { Module, Provides, Singleton } from "tsdi";
import { Config } from "./config";

// Async @Provides demonstrating the M12 shape: the method is `async`
// and returns `Promise<Config>`. tsdi unwraps the `Promise<T>` for
// the binding key (so consumers see Config, not Promise<Config>) and
// awaits the value during the dagger's `_resolve` phase.
@Module
export class ConfigModule {
  @Singleton
  @Provides
  static async loadConfig(): Promise<Config> {
    // Pretend this is `await fs.readFile("config.json")`.
    return Promise.resolve(new Config("postgres://localhost/app", 60_000));
  }
}
