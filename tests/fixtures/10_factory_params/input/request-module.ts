import { Module, Provides } from "@msulak/anvil";
import { HttpRequest } from "./http";
import { RequestContext } from "./request-context";

// @Provides method that consumes the `req: HttpRequest` factory param
// and produces the request-scoped RequestContext. This is the key M11
// shape: a binding whose dependency is a runtime-supplied value.
@Module
export class RequestModule {
  @Provides
  static context(req: HttpRequest): RequestContext {
    return new RequestContext(req.url, req.headers["authorization"]);
  }
}
