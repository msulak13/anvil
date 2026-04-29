import { Inject } from "tsdi";
import { Heater } from "./heater";

@Inject
export class ElectricHeater implements Heater {
  heat(): void {
    // pretend to heat
  }
}
