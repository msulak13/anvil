import { Subcomponent } from "tsdi";
import { RequestModule } from "./request-module";
import { Handler } from "./handler";

// Per-request scope. Reached only through AppComponent's
// `requestComponent(req, res)` factory — never standalone. Both the
// `req: HttpRequest` and `res: HttpResponse` factory parameters
// declared on the parent become virtual bindings inside this graph,
// satisfying:
//   - RequestModule.context(req) → RequestContext (one fresh ctx per call)
//   - Handler's `res: HttpResponse` ctor parameter
@Subcomponent({ modules: [RequestModule] })
export abstract class RequestComponent {
  abstract handler(): Handler;
}
