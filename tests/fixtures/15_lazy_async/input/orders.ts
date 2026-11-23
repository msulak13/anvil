import { Inject, Lazy, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { Payments } from "./payments";

// `_resolve` must assign Orders before Payments, which reads it eagerly.
@Inject
@Singleton
export class Orders {
  constructor(
    readonly db: Database,
    readonly payments: Lazy<Payments>,
  ) {}
}
