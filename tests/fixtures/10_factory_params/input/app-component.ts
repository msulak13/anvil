import { Component } from "tsdi";
import { HttpRequest, HttpResponse } from "./http";
import { RequestComponent } from "./request-component";

// Root of the dagger graph. The `requestComponent(req, res)` method is
// the heart of M11 — its parameters become virtual bindings inside the
// child graph. Each HTTP request gets a fresh RequestComponent (and
// thus a fresh RequestContext, Handler, etc.) without any of those
// types being threaded manually.
@Component({ modules: [] })
export abstract class AppComponent {
  abstract requestComponent(req: HttpRequest, res: HttpResponse): RequestComponent;
}
