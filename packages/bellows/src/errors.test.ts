import type { NextFunction, Request, Response } from "express";
import { describe, expect, it, vi } from "vitest";
import {
  BadGatewayError,
  BadRequestError,
  ConflictError,
  ForbiddenError,
  GatewayTimeoutError,
  GoneError,
  HttpError,
  InternalServerError,
  MethodNotAllowedError,
  NotAcceptableError,
  NotFoundError,
  NotImplementedError,
  PayloadTooLargeError,
  PreconditionFailedError,
  RequestTimeoutError,
  ServiceUnavailableError,
  TooManyRequestsError,
  UnauthorizedError,
  UnprocessableEntityError,
  UnsupportedMediaTypeError,
  errorHandler,
} from "./errors.js";

// --- HttpError ---

describe("HttpError", () => {
  it("uses `error` as the message when none is given", () => {
    const err = new HttpError(400, "Bad Request");
    expect(err.message).toBe("Bad Request");
    expect(err.body()).toEqual({ error: "Bad Request" });
  });

  it("includes a distinct message in the body", () => {
    const err = new HttpError(400, "Bad Request", "the `id` field is required");
    expect(err.body()).toEqual({
      error: "Bad Request",
      message: "the `id` field is required",
    });
  });

  it("sets `name` to the concrete subclass name", () => {
    const err = new NotFoundError();
    expect(err.name).toBe("NotFoundError");
    expect(err).toBeInstanceOf(HttpError);
    expect(err).toBeInstanceOf(Error);
  });
});

// --- Subclasses map to the right status/error pair ---

describe("HttpError subclasses", () => {
  const cases: Array<[new (message?: string) => HttpError, number, string]> = [
    [BadRequestError, 400, "Bad Request"],
    [UnauthorizedError, 401, "Unauthorized"],
    [ForbiddenError, 403, "Forbidden"],
    [NotFoundError, 404, "Not Found"],
    [MethodNotAllowedError, 405, "Method Not Allowed"],
    [NotAcceptableError, 406, "Not Acceptable"],
    [RequestTimeoutError, 408, "Request Timeout"],
    [ConflictError, 409, "Conflict"],
    [GoneError, 410, "Gone"],
    [PreconditionFailedError, 412, "Precondition Failed"],
    [PayloadTooLargeError, 413, "Payload Too Large"],
    [UnsupportedMediaTypeError, 415, "Unsupported Media Type"],
    [UnprocessableEntityError, 422, "Unprocessable Entity"],
    [TooManyRequestsError, 429, "Too Many Requests"],
    [InternalServerError, 500, "Internal Server Error"],
    [NotImplementedError, 501, "Not Implemented"],
    [BadGatewayError, 502, "Bad Gateway"],
    [ServiceUnavailableError, 503, "Service Unavailable"],
    [GatewayTimeoutError, 504, "Gateway Timeout"],
  ];

  it.each(cases)("%s has status %d and error %j", (Ctor, status, error) => {
    const err = new Ctor();
    expect(err.status).toBe(status);
    expect(err.error).toBe(error);
    expect(err.body()).toEqual({ error });
  });

  it("passes through a custom message", () => {
    const err = new NotFoundError("no user with that id");
    expect(err.body()).toEqual({ error: "Not Found", message: "no user with that id" });
  });
});

// --- errorHandler ---

function mockRes(): Response {
  const res = {
    headersSent: false,
    status: vi.fn().mockReturnThis(),
    json: vi.fn().mockReturnThis(),
    end: vi.fn(),
  };
  return res as unknown as Response;
}

describe("errorHandler", () => {
  it("maps an HttpError to its status and body", () => {
    const res = mockRes();
    const req = {} as Request;
    errorHandler(new ForbiddenError("nope"), req, res, vi.fn() as NextFunction);

    expect(res.status).toHaveBeenCalledWith(403);
    expect(res.json).toHaveBeenCalledWith({ error: "Forbidden", message: "nope" });
  });

  it("falls back to a generic 500 for unknown errors", () => {
    const res = mockRes();
    const req = {} as Request;
    errorHandler(new Error("some internal detail"), req, res, vi.fn() as NextFunction);

    expect(res.status).toHaveBeenCalledWith(500);
    expect(res.json).toHaveBeenCalledWith({
      error: "Internal Server Error",
      message: "Internal server error",
    });
  });

  it("logs unknown errors via req.log when present", () => {
    const res = mockRes();
    const log = { error: vi.fn() };
    const req = { log } as unknown as Request;
    const err = new Error("boom");
    errorHandler(err, req, res, vi.fn() as NextFunction);

    expect(log.error).toHaveBeenCalledWith({ err }, "Unhandled error");
  });

  it("does not throw when req.log is absent", () => {
    const res = mockRes();
    const req = {} as Request;
    expect(() => errorHandler(new Error("boom"), req, res, vi.fn() as NextFunction)).not.toThrow();
  });

  it("ends the response without writing a body if headers are already sent", () => {
    const res = mockRes();
    (res as { headersSent: boolean }).headersSent = true;
    const req = {} as Request;
    errorHandler(new NotFoundError(), req, res, vi.fn() as NextFunction);

    expect(res.end).toHaveBeenCalled();
    expect(res.status).not.toHaveBeenCalled();
  });
});
