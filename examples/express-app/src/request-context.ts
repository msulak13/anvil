// Per-request derived state. Built fresh inside each RequestComponent
// from the `req` factory parameter — never cached, never shared across
// requests.

export class RequestContext {
  constructor(
    public readonly path: string,
    public readonly method: string,
    public readonly userId: number | undefined,
    public readonly requestId: string,
  ) {}
}
