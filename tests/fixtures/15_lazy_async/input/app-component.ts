import { Component, Singleton } from "@anvil-di/anvil";
import { DatabaseModule } from "./database-module";
import { Orders } from "./orders";
import { Payments } from "./payments";

@Singleton
@Component({ modules: [DatabaseModule] })
export abstract class AppComponent {
  abstract orders(): Orders;
  abstract payments(): Payments;
}
