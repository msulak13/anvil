import { Inject } from "@anvil-di/anvil";
import { Greeter } from "./greeter";
import { HttpRequest } from "./http";

// Reaches the root's constructor parameter through the parent's `Greeter`.
@Inject
export class Handler {
  constructor(
    private greeter: Greeter,
    private req: HttpRequest,
  ) {}

  handle(): string {
    return this.greeter.greet(this.req.path);
  }
}
