import type { AuthnResult, AuthnService, AuthzDecision, AuthzService } from "@anvil-di/bellows";

export interface User {
  id: string;
}

export class SessionAuthn implements AuthnService<User, "bearerAuth"> {
  identify(req: unknown): AuthnResult<User> {
    return { identified: false };
  }
}

export class RoleAuthz implements AuthzService {
  authorize(req: unknown, user: unknown): AuthzDecision {
    return "next";
  }
}
