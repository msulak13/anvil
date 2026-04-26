import { Inject, Singleton } from "tsdi";

@Singleton
@Inject
export class Heater {
  constructor() {}
  on() { /* noop */ }
}
