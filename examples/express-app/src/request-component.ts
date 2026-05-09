import { Subcomponent } from "@msulak/anvil";
import { RequestModule } from "./request-module.js";
import { UserController } from "./user-controller.js";
import { RequestContext } from "./request-context.js";

// Per-request graph. Reached only through AppComponent's
// `requestComponent(req, res)` factory; each call yields a fresh
// dagger with its own `req` / `res` fields and request-scoped state.
//
// Bindings inside this graph can request:
//   - Request           (factory parameter, supplied at call site)
//   - Response          (factory parameter, supplied at call site)
//   - RequestContext    (built fresh by RequestModule.context(req))
//   - UserRepository    (inherited from parent — same instance every request)
//   - Logger            (inherited from parent)
@Subcomponent({ modules: [RequestModule] })
export abstract class RequestComponent {
  abstract userController(): UserController;
  abstract context(): RequestContext;
}
