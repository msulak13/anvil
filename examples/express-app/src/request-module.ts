import { Module, Provides } from "@anvil-di/anvil";
import type { Request } from "express";
import { RequestContext } from "./request-context.js";

// @Provides method whose argument is THE FACTORY PARAMETER — the
// dagger threads `req` from `requestComponent(req, res)` through here.
// Each call to `dagger.requestComponent(...).context()` builds a fresh
// RequestContext from that request's headers and URL.
@Module
export class RequestModule {
  @Provides
  static context(req: Request): RequestContext {
    const userIdHeader = req.header("x-user-id");
    const userId = userIdHeader === undefined ? undefined : Number(userIdHeader);
    const requestId = req.header("x-request-id") ?? crypto.randomUUID();
    return new RequestContext(
      req.path,
      req.method,
      Number.isNaN(userId) ? undefined : userId,
      requestId,
    );
  }
}
