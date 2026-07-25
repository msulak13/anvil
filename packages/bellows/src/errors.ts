import type { NextFunction, Request, Response } from "express";

/**
 * HTTP error hierarchy for bellows controllers.
 *
 * bellows-generated route handlers do not catch exceptions, and Express 5
 * forwards rejected promises from async handlers to the error middleware.
 * So controller methods signal non-2xx outcomes by throwing one of these,
 * and `errorHandler` (registered last on the app) maps them to a response.
 */
export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly error: string,
    message?: string,
  ) {
    super(message ?? error);
    this.name = new.target.name;
  }

  body(): Record<string, unknown> {
    return this.message && this.message !== this.error
      ? { error: this.error, message: this.message }
      : { error: this.error };
  }
}

export class BadRequestError extends HttpError {
  constructor(message?: string) {
    super(400, "Bad Request", message);
  }
}

export class UnauthorizedError extends HttpError {
  constructor(message?: string) {
    super(401, "Unauthorized", message);
  }
}

export class ForbiddenError extends HttpError {
  constructor(message?: string) {
    super(403, "Forbidden", message);
  }
}

export class NotFoundError extends HttpError {
  constructor(message?: string) {
    super(404, "Not Found", message);
  }
}

export class MethodNotAllowedError extends HttpError {
  constructor(message?: string) {
    super(405, "Method Not Allowed", message);
  }
}

export class NotAcceptableError extends HttpError {
  constructor(message?: string) {
    super(406, "Not Acceptable", message);
  }
}

export class RequestTimeoutError extends HttpError {
  constructor(message?: string) {
    super(408, "Request Timeout", message);
  }
}

export class ConflictError extends HttpError {
  constructor(message?: string) {
    super(409, "Conflict", message);
  }
}

export class GoneError extends HttpError {
  constructor(message?: string) {
    super(410, "Gone", message);
  }
}

export class PreconditionFailedError extends HttpError {
  constructor(message?: string) {
    super(412, "Precondition Failed", message);
  }
}

export class PayloadTooLargeError extends HttpError {
  constructor(message?: string) {
    super(413, "Payload Too Large", message);
  }
}

export class UnsupportedMediaTypeError extends HttpError {
  constructor(message?: string) {
    super(415, "Unsupported Media Type", message);
  }
}

export class UnprocessableEntityError extends HttpError {
  constructor(message?: string) {
    super(422, "Unprocessable Entity", message);
  }
}

export class TooManyRequestsError extends HttpError {
  constructor(message?: string) {
    super(429, "Too Many Requests", message);
  }
}

export class InternalServerError extends HttpError {
  constructor(message?: string) {
    super(500, "Internal Server Error", message);
  }
}

export class NotImplementedError extends HttpError {
  constructor(message?: string) {
    super(501, "Not Implemented", message);
  }
}

export class BadGatewayError extends HttpError {
  constructor(message?: string) {
    super(502, "Bad Gateway", message);
  }
}

export class ServiceUnavailableError extends HttpError {
  constructor(message?: string) {
    super(503, "Service Unavailable", message);
  }
}

export class GatewayTimeoutError extends HttpError {
  constructor(message?: string) {
    super(504, "Gateway Timeout", message);
  }
}

/**
 * Express error-handling middleware. Maps `HttpError` subclasses to their
 * status and body.
 *
 * Only handles errors it recognizes as its own — anything that isn't an
 * instance of this module's `HttpError` is forwarded via `next(err)` rather
 * than converted to a generic 500. This is what makes it safe for
 * `bellowsRoutes()` to install by default: a consumer with its own
 * `HttpError`-like hierarchy (a natural thing to build, since bellows itself
 * had no error-handling primitives before this class existed) can mount
 * their own app-level handler after `bellowsRoutes()` and still see every
 * error that isn't a bellows `HttpError` — this handler only ever intercepts
 * the ones it can actually map.
 *
 * Must be registered after all routes.
 */
export function errorHandler(
  err: unknown,
  _req: Request,
  res: Response,
  next: NextFunction,
): void {
  if (res.headersSent) {
    // Response already streaming (e.g. SSE) — let Express close the socket.
    res.end();
    return;
  }

  if (err instanceof HttpError) {
    res.status(err.status).json(err.body());
    return;
  }

  next(err);
}
