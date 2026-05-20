import { Component } from "@anvil-di/anvil";
import { Pump } from "./pump";
import { Heater } from "./heater";

@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
  abstract heater(): Heater;
}
