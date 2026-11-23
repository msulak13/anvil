import { Inject, Singleton } from "@anvil-di/anvil";
import { Parent } from "./parent";

@Inject
@Singleton
export class Child {
  constructor(readonly parent: Parent) {}
}
