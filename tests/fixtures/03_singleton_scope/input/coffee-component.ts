import { Component, Singleton } from "tsdi";
import { Pump } from "./pump";
import { Heater } from "./heater";

@Singleton
@Component({ modules: [] })
export abstract class CoffeeShop {
  abstract pump(): Pump;
  abstract heater(): Heater;
}
