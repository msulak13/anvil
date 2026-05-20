import { Inject } from "@anvil-di/anvil";
import { Heater } from "./heater";

@Inject
export class RequestHandler {
  constructor(private heater: Heater) {}
  handle(): void {
    this.heater.on();
  }
}
