import { Inject, Lazy, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { Payments } from "./payments";

// Payments is built on the first `payments.get()`, not by `createAppComponent()`.
@Inject
@Singleton
export class Orders {
  constructor(
    readonly db: Database,
    readonly payments: Lazy<Payments>,
  ) {}
}
