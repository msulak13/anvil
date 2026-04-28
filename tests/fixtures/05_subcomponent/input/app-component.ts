import { Component, Singleton } from "tsdi";
import { Heater } from "./heater";
import { RequestComponent } from "./request-component";

@Singleton
@Component({ modules: [] })
export abstract class AppComponent {
  abstract heater(): Heater;
  abstract requestComponent(): RequestComponent;
}
