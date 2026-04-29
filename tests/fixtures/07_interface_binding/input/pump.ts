import { Inject } from "tsdi";
import { Heater } from "./heater";

// Consumer takes the *interface* as a constructor dep. The generated
// dagger calls `getHeater()` which the @Binds method routes to the
// concrete `ElectricHeater` factory.
@Inject
export class Pump {
  constructor(private heater: Heater) {}

  pump(): void {
    this.heater.heat();
  }
}
