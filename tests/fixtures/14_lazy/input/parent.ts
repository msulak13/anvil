import { Inject, Lazy, Singleton } from "@anvil-di/anvil";
import { Child } from "./child";

// `Lazy` closes the Parent → Child → Parent cycle.
@Inject
@Singleton
export class Parent {
  constructor(readonly child: Lazy<Child>) {}
}
