import { Inject, Singleton } from "@anvil-di/anvil";

@Singleton
@Inject
export class Heater {
  constructor() {}
  on() { /* noop */ }
}
