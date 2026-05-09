import { Inject } from "@msulak/anvil";
import { Heater } from "./heater";

@Inject
export class Pump {
  constructor(private heater: Heater) {}
  pump() { this.heater.on(); }
}
