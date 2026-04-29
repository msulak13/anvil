import { Component } from "tsdi";
import { HeaterModule } from "./heater-module";
import { Heater } from "./heater";
import { Pump } from "./pump";

@Component({ modules: [HeaterModule] })
export abstract class CoffeeShop {
  // Entry point exposes the interface — callers see only the interface,
  // never the concrete `ElectricHeater`.
  abstract heater(): Heater;
  abstract pump(): Pump;
}
