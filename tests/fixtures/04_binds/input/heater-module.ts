import { Module, Binds } from "@anvil-di/anvil";
import { Heater } from "./heater";
import { ElectricHeater } from "./electric-heater";

@Module
export class HeaterModule {
  @Binds static bindHeater(impl: ElectricHeater): Heater {
    return impl;
  }
}
