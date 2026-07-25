import type { JSONSchema7 } from "json-schema";

export type { JSONSchema7 };

export interface Validator<T> {
  safeParse(input: unknown): { success: true; data: T } | { success: false; error: unknown };
  jsonSchema?(): JSONSchema7;
}

/**
 * Decodes a non-JSON, non-form request body into the value that `S` then
 * validates via `Body<S, C>`. `contentType` must be a literal-initialized
 * property (not a getter) — same constraint as `ResponseCodec`, for the same
 * reason: codegen resolves it statically to build the `OpenAPI` spec (runtime
 * body-parser selection always reads `contentType`/calls `decode` off the
 * object itself).
 */
export interface RequestCodec<T> {
  readonly contentType: string;
  decode(raw: Buffer): T;
}

/**
 * Validates `req.body` against `S`. The optional second type argument `C`
 * decodes the raw request body with a `RequestCodec` first instead of
 * assuming JSON — `bellowsRoutes` mounts `express.raw()` scoped to
 * `C.contentType` for the route and calls `C.decode()` before validation.
 * `C` must decode into exactly the type `S` validates — this is enforced at
 * compile time by the `RequestCodec<Body<S>>` bound.
 *
 * ```ts
 * const twimlRequestCodec: RequestCodec<GatherCallback> = {
 *   contentType: "application/xml",
 *   decode: parseTwimlRequest,
 * };
 *
 * @Post("/webhooks/gather")
 * gather(body: Body<typeof GatherCallbackSchema, typeof twimlRequestCodec>): void { ... }
 * ```
 */
export type Body<
  S extends Validator<unknown>,
  C extends RequestCodec<S extends Validator<infer T> ? T : never> = never,
> = S extends Validator<infer T> ? T : never;
export type Query<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Params<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;
export type Responds<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;

/**
 * `application/x-www-form-urlencoded` counterpart to `Body<S>` — validates
 * `req.body` the same way, but tells codegen/OpenAPI the route expects a form
 * body instead of JSON, and to mount `express.urlencoded()` for the route.
 */
export type FormBody<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;

/** Validates `req.headers` against `S`. Keys must be lower-case, matching
 *  Node/Express's own header casing (e.g. `"x-twilio-signature"`). */
export type Headers<S extends Validator<unknown>> = S extends Validator<infer T> ? T : never;

/**
 * Injects the raw, unparsed request body bytes captured before any body
 * parsing. `bellowsRoutes` populates `req.rawBody` for any route that
 * declares this param, via a `verify` callback on whichever body-parser it
 * mounts for the route (`express.raw()` if `RawBody` is the only body param
 * declared, `express.json()`/`express.urlencoded()` if paired with `Body<S>`/
 * `FormBody<S>`).
 */
export type RawBody = Buffer;

/**
 * Serializes a validated `Responds<S>` value into a non-JSON response body.
 * `contentType` must be a literal-initialized property (not a getter) —
 * codegen resolves it statically to build the `OpenAPI` spec, so a computed
 * value can't be read at build time.
 */
export interface ResponseCodec<T> {
  readonly contentType: string;
  encode(value: T): string | Buffer;
}

/**
 * Like `Responds<S>`, but serializes the validated return value with `C`
 * instead of `res.json()`. `C` must encode exactly the type `S` validates —
 * this is enforced at compile time by the `ResponseCodec<Responds<S>>` bound.
 *
 * ```ts
 * const twimlCodec: ResponseCodec<TwimlResponse> = {
 *   contentType: "application/xml",
 *   encode: renderTwiml,
 * };
 *
 * @Post("/webhooks/gather")
 * async gather(req: Request): Promise<Produces<typeof TwimlResponseSchema, typeof twimlCodec>> { ... }
 * ```
 */
export type Produces<
  S extends Validator<unknown>,
  C extends ResponseCodec<Responds<S>>,
> = Responds<S>;

export function withJsonSchema<T>(
  validator: Validator<T>,
  schema: JSONSchema7,
): Validator<T> & { jsonSchema(): JSONSchema7 } {
  return {
    safeParse: (input) => validator.safeParse(input),
    jsonSchema: () => schema,
  };
}
