// A request-scoped value derived from the incoming request. The
// @Provides method below consumes the HttpRequest factory-param and
// builds this RequestContext fresh for each call, demonstrating that
// the dagger threads runtime values through the binding graph.
export class RequestContext {
  constructor(
    public readonly path: string,
    public readonly auth: string | undefined,
  ) {}
}
