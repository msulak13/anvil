// Transport-agnostic route registration. Controllers describe their
// routes by calling the supplied `register` callback; the server
// implementation (Express, Fastify, http, …) decides how to bind it.
//
// Keeping Express types out of the dagger graph means:
//   1. Controllers are unit-testable without booting Express.
//   2. Swapping HTTP frameworks doesn't ripple through every binding.
//   3. The fixture's parser walk doesn't need to resolve `express` —
//      that import lives only in `server.ts`, outside the component
//      graph.

export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

// `unknown` here keeps the dagger Express-free; `server.ts` casts to
// the framework's request/response types when wiring the handler.
export type Handler = (req: unknown, res: unknown) => void;

export type RegisterFn = (method: Method, path: string, handler: Handler) => void;

export interface RouteRegistrar {
  register(register: RegisterFn): void;
}
