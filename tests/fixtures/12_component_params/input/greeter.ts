import { Inject, Singleton } from "@anvil-di/anvil";
import { Config } from "./config";

@Inject
@Singleton
export class Greeter {
  constructor(private config: Config) {}

  greet(name: string): string {
    return `${this.config.greeting}, ${name}`;
  }
}
