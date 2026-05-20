import { Component, Singleton } from "@anvil-di/anvil";
import { Pump } from "./pump";
import { Heater } from "./heater";

@Singleton
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
  abstract heater(): Heater;
}
