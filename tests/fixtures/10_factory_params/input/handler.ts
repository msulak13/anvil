import { Inject } from "tsdi";
import { HttpResponse } from "./http";
import { RequestContext } from "./request-context";

// Lives inside the request subcomponent; gets the RequestContext (built
// from the factory-param `req`) and the response factory-param itself.
@Inject
export class Handler {
  constructor(
    private ctx: RequestContext,
    private res: HttpResponse,
  ) {}

  handle(): void {
    if (this.ctx.auth === undefined) {
      this.res.send(401, "unauthorized");
      return;
    }
    this.res.send(200, `hello from ${this.ctx.path}`);
  }
}
