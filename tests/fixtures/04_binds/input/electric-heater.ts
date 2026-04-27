import { Inject } from "tsdi";
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
