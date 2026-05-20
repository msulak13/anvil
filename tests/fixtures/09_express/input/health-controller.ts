import { Inject, Singleton } from "@anvil-di/anvil";
import { RegisterFn, RouteRegistrar } from "./route-registrar";

// A second registrar to demonstrate that @IntoSet collects all
// contributions automatically — adding a controller is one
// `@IntoSet @Provides` line in `AppModule`, no central registry to edit.
@Inject
@Singleton
export class HealthController implements RouteRegistrar {
  register(register: RegisterFn): void {
    register("GET", "/healthz", this.healthz);
  }

  private healthz = (_req: unknown, res: unknown): void => {
    const r = res as { json(b: unknown): void };
    r.json({ ok: true });
  };
}
