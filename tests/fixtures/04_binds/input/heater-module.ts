import { Module, Binds } from "tsdi";
import { Heater } from "./heater";
import { ElectricHeater } from "./electric-heater";

@Module
export class HeaterModule {
  @Binds static bindHeater(impl: ElectricHeater): Heater {
    return impl;
  }
}
