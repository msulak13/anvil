import { Module, Binds } from "@msulak/anvil";
import { Heater } from "./heater";
import { ElectricHeater } from "./electric-heater";

@Module
export class HeaterModule {
  @Binds static bindHeater(impl: ElectricHeater): Heater {
    return impl;
  }
}
