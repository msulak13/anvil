import { Inject } from "@anvil-di/anvil";
import { Heater } from "./heater";

@Inject
export class ElectricHeater extends Heater {
  constructor() {
    super();
  }
  on(): void {
    /* noop */
  }
}
