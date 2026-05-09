import { Inject, Singleton } from "@msulak/anvil";

@Singleton
@Inject
export class Heater {
  constructor() {}
  on() { /* noop */ }
}
