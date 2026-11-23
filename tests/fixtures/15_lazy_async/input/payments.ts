import { Inject, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { Orders } from "./orders";

@Inject
@Singleton
export class Payments {
  constructor(
    readonly db: Database,
    readonly orders: Orders,
  ) {}
}
