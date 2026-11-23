import { Component, Singleton } from "@anvil-di/anvil";
import { Clock } from "./clock";
import { Config } from "./config";
import { Greeter } from "./greeter";
import { HttpRequest } from "./http";
import { RequestComponent } from "./request-component";

// `config` is read by bindings and gets a private field on the dagger;
// `clock` is only kept by this class, so the dagger forwards it to `super`
// and stores nothing.
@Singleton
@Component({ modules: [] })
export abstract class AppComponent {
  constructor(
    readonly config: Config,
    protected readonly clock: Clock,
  ) {}

  abstract greeter(): Greeter;
  abstract requestComponent(req: HttpRequest): RequestComponent;
}
