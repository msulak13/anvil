import { Authn, Authz, Middleware, Returns, Tag, bellowsCompose } from "@anvil-di/bellows";
import { auditLog, requireAuth } from "../auth-middleware";
import { RoleAuthz, SessionAuthn } from "../auth-services";

export const Authenticated = bellowsCompose(Authn(SessionAuthn), Middleware(requireAuth));

export const AdminArea = bellowsCompose(Authenticated, Tag("admin"));

export const Audited = bellowsCompose(Authz(RoleAuthz), Middleware(auditLog), Returns(403));
