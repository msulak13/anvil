import { Inject } from "@anvil-di/anvil";
import type { Response } from "express";
import type { Logger } from "./logger.js";
import { RequestContext } from "./request-context.js";
import { UserRepository } from "./user-repository.js";

// Per-request controller. Constructor takes:
//   - RequestContext  (request-scoped, built fresh in this graph)
//   - Response        (factory parameter — the actual Express res)
//   - Logger          (app-scoped — INHERITED from the parent dagger)
//   - UserRepository  (app-scoped — INHERITED from the parent dagger)
//
// The dagger threads each through the right factory: parent for
// inherited bindings, child for request-scoped ones. The user code
// stays oblivious — it just declares ctor deps and the codegen does
// the wiring.
@Inject
export class UserController {
  constructor(
    private ctx: RequestContext,
    private res: Response,
    private log: Logger,
    private users: UserRepository,
  ) {}

  list(): void {
    this.log.info("UserController.list", { requestId: this.ctx.requestId });
    this.res.json(this.users.list());
  }

  byId(idParam: string): void {
    const id = Number(idParam);
    if (!Number.isFinite(id)) {
      this.res.status(400).json({ error: "id must be a number" });
      return;
    }
    const user = this.users.byId(id);
    if (user === undefined) {
      this.log.warn("UserController.byId not_found", {
        requestId: this.ctx.requestId,
        id,
      });
      this.res.status(404).json({ error: "user not found" });
      return;
    }
    this.log.info("UserController.byId", {
      requestId: this.ctx.requestId,
      id,
    });
    this.res.json(user);
  }

  create(name: unknown, email: unknown): void {
    if (typeof name !== "string" || typeof email !== "string") {
      this.res.status(400).json({ error: "name and email must be strings" });
      return;
    }
    const user = this.users.create(name, email);
    this.log.info("UserController.create", {
      requestId: this.ctx.requestId,
      id: user.id,
    });
    this.res.status(201).json(user);
  }

  whoami(): void {
    if (this.ctx.userId === undefined) {
      this.res.status(401).json({ error: "missing x-user-id header" });
      return;
    }
    const user = this.users.byId(this.ctx.userId);
    if (user === undefined) {
      this.res.status(401).json({ error: "unknown user" });
      return;
    }
    this.res.json({ requestId: this.ctx.requestId, user });
  }
}
