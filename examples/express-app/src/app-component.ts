import { Component, Singleton } from "@anvil-di/anvil";
import type { Request, Response } from "express";
import { AppModule } from "./app-module.js";
import type { Logger } from "./logger.js";
import { RequestComponent } from "./request-component.js";

// Root of the dagger graph. @Singleton because the singletons inside
// (ConsoleLogger, UserRepository) require it.
//
// `requestComponent(req: Request, res: Response): RequestComponent` is
// the factory-parameter entry point — calling it yields a fresh
// per-request dagger threaded with the actual Express req/res.
@Singleton
@Component({ modules: [AppModule] })
export abstract class AppComponent {
  abstract logger(): Logger;
  abstract requestComponent(req: Request, res: Response): RequestComponent;
}
