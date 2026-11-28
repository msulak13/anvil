import { describe, expect, it } from "vitest";
import { Authn, Middleware, Returns, Tag, bellowsCompose } from "./decorators.js";
import type { AuthnResult, AuthnService } from "./authz.js";

class NoopAuthn implements AuthnService<{ id: string }, "bearerAuth"> {
  identify(): AuthnResult<{ id: string }> {
    return { identified: false };
  }
}

const noopMiddleware = (_req: unknown, _res: unknown, next: () => void): void => next();

const Authenticated = bellowsCompose(Authn(NoopAuthn), Returns(401));
const AdminRoute = bellowsCompose(Authenticated, Middleware(noopMiddleware), Tag("admin"));

@AdminRoute
class ComposedController {
  @Authenticated
  handler(): void {}

  @Authenticated
  static staticHandler(): void {}
}

export function callFormIsRejected(): unknown {
  // @ts-expect-error a composite is applied bare, not called
  @AdminRoute()
  class CalledComposite {}
  return CalledComposite;
}

describe("bellowsCompose", () => {
  it("applies at class, method and static method level as a no-op", () => {
    expect(new ComposedController()).toBeInstanceOf(ComposedController);
  });
});
