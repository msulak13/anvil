import { Inject, Singleton } from "tsdi";
import { Logger } from "./logger";
import { RegisterFn, RouteRegistrar } from "./route-registrar";
import { UserService } from "./user-service";

// @Singleton because Express invokes handlers across many requests; we
// want one controller instance owning the cached service refs.
//
// Handlers are arrow-function fields so `this` is bound when Express
// calls them as plain functions (`app.get(path, controller.list)`).
@Inject
@Singleton
export class UserController implements RouteRegistrar {
  constructor(
    private users: UserService,
    private log: Logger,
  ) {}

  register(register: RegisterFn): void {
    register("GET", "/users", this.list);
    register("GET", "/users/:id", this.byId);
  }

  private list = (_req: unknown, res: unknown): void => {
    const r = res as { json(b: unknown): void };
    r.json(this.users.list());
  };

  private byId = (req: unknown, res: unknown): void => {
    const q = req as { params: { id: string } };
    const r = res as {
      json(b: unknown): void;
      status(c: number): { end(): void };
    };
    const found = this.users.byId(Number(q.params.id));
    if (found === undefined) {
      r.status(404).end();
      return;
    }
    r.json(found);
    this.log.info(`served user ${found.id}`);
  };
}
