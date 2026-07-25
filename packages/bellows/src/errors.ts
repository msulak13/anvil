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
 * Minimal structural type for a pino-style logger. `errorHandler` looks for
 * `req.log` (as attached by e.g. `pino-http`) but does not depend on pino —
 * anything satisfying this shape works, and unhandled errors are still
 * caught if `req.log` is absent.
 */
export interface RequestLogger {
  error(obj: Record<string, unknown>, msg?: string): void;
}

export interface RequestWithLogger extends Request {
  log?: RequestLogger;
}

/**
 * Express error-handling middleware. Maps `HttpError` subclasses to their
 * status and body. Falls back to 500 for anything unexpected, logging the
 * full error via `req.log` (if present) while returning only a generic
 * message to the caller — internal error text should never be echoed back,
 * since it may embed request data the caller isn't entitled to see.
 *
 * Must be registered after all routes.
 */
export function errorHandler(
  err: unknown,
  req: Request,
  res: Response,
  _next: NextFunction,
): void {
  // `_next` is required so Express recognizes this as error-handling middleware.
  void _next;
  if (res.headersSent) {
    // Response already streaming (e.g. SSE) — let Express close the socket.
    res.end();
    return;
  }

  if (err instanceof HttpError) {
    res.status(err.status).json(err.body());
    return;
  }

  (req as RequestWithLogger).log?.error({ err }, "Unhandled error");
  res.status(500).json({ error: "Internal Server Error", message: "Internal server error" });
}
