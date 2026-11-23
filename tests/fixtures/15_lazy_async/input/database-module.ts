import { Module, Provides, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";

@Module
export class DatabaseModule {
  @Singleton
  @Provides
  static async open(): Promise<Database> {
    return new Database("pg://db");
  }
}
