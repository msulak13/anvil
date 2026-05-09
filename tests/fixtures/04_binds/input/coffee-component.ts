import { Component } from "@msulak/anvil";
import { HeaterModule } from "./heater-module";
import { Heater } from "./heater";

@Component({ modules: [HeaterModule] })
export abstract class CoffeeShop {
  abstract heater(): Heater;
}
