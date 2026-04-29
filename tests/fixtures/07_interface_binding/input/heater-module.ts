import { Module, Binds } from "tsdi";
import { Heater } from "./heater";
import { ElectricHeater } from "./electric-heater";

@Module
export class HeaterModule {
  // Stage-3 decorators can't decorate abstract methods, so @Binds is a
  // static method with a trivial body. tsdi-codegen ignores the body
  // and emits `getHeater()` as a delegate to `getElectricHeater()`.
  @Binds
  static bindHeater(impl: ElectricHeater): Heater {
    return impl;
  }
}
