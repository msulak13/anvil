import { Inject } from "@anvil-di/anvil";
import { Heater } from "./heater";

@Inject
export class ElectricHeater implements Heater {
  heat(): void {
    // pretend to heat
  }
}
