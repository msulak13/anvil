import { describe, expect, it } from "vitest";
import { Authn, Authz, Middleware, Security } from "./decorators.js";
import type { AuthnResult, AuthnService, AuthzDecision, AuthzService } from "./authz.js";

// Regression test for a TS decorator-overload bug: `Authn`/`Authz`/`Middleware`/
// `Security` must typecheck when applied as BOTH a class decorator and a
// method decorator in the same compilation unit. Overloaded signatures with
// identical outer parameter lists can't be disambiguated by TS's decorator
// checker (it always binds the first match regardless of target), so these
// decorators are intentionally loosely typed — this file exists to catch a
// regression to typed overloads at compile time.

class NoopAuthn implements AuthnService<{ id: string }, "bearerAuth"> {
  identify(): AuthnResult<{ id: string }> {
    return { identified: false };
  }
}

class NoopAuthz implements AuthzService {
  authorize(): AuthzDecision {
    return "next";
  }
}

const noopMiddleware = (_req: unknown, _res: unknown, next: () => void): void => next();

@Authn(NoopAuthn)
@Authz(NoopAuthz)
@Middleware(noopMiddleware)
@Security("bearerAuth")
class ClassLevelController {
  @Authn(NoopAuthn)
  @Authz(NoopAuthz)
  @Middleware(noopMiddleware)
  @Security("bearerAuth")
  handler(): void {}
}

describe("dual class/method decorators", () => {
  it("compiles and constructs at both levels", () => {
    expect(new ClassLevelController()).toBeInstanceOf(ClassLevelController);
  });
});
