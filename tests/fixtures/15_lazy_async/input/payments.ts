import { Inject, Singleton } from "@anvil-di/anvil";
import { Database } from "./database";
import { Orders } from "./orders";

@Inject
@Singleton
export class Payments {
  static constructed = 0;

  constructor(
    readonly db: Database,
    readonly orders: Orders,
  ) {
    Payments.constructed++;
  }
}
