import { EventEmitter } from "node:events";
import type { Request, Response } from "express";
import { describe, expect, it, vi } from "vitest";
import { disconnectSignal, SseStream } from "./sse.js";

function mockReq(): Request {
  return new EventEmitter() as unknown as Request;
}

function mockRes(): Response {
  const res = {
    headersSent: false,
    status: vi.fn().mockReturnThis(),
    setHeader: vi.fn().mockReturnThis(),
    flushHeaders: vi.fn(),
    write: vi.fn(),
    end: vi.fn(),
  };
  return res as unknown as Response;
}

describe("disconnectSignal", () => {
  it("is not aborted before the request closes", () => {
    const req = mockReq();
    const signal = disconnectSignal(req);
    expect(signal.aborted).toBe(false);
  });

  it("aborts when the request emits close", () => {
    const req = mockReq();
    const signal = disconnectSignal(req);
    (req as unknown as EventEmitter).emit("close");
    expect(signal.aborted).toBe(true);
  });
});

describe("SseStream", () => {
  it("open() sets SSE headers and flushes them", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.open();

    expect(res.status).toHaveBeenCalledWith(200);
    expect(res.setHeader).toHaveBeenCalledWith("Content-Type", "text/event-stream; charset=utf-8");
    expect(res.setHeader).toHaveBeenCalledWith("Cache-Control", "no-cache, no-transform");
    expect(res.setHeader).toHaveBeenCalledWith("Connection", "keep-alive");
    expect(res.flushHeaders).toHaveBeenCalled();
  });

  it("open() is a no-op if headers were already sent", () => {
    const res = mockRes();
    (res as unknown as { headersSent: boolean }).headersSent = true;
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.open();

    expect(res.status).not.toHaveBeenCalled();
    expect(res.flushHeaders).not.toHaveBeenCalled();
  });

  it("send() frames a string payload as a data line", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.send("hello");

    expect(res.write).toHaveBeenCalledWith("data: hello\n\n");
  });

  it("send() JSON-encodes non-string payloads", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.send({ progress: 42 });

    expect(res.write).toHaveBeenCalledWith('data: {"progress":42}\n\n');
  });

  it("send() includes event/id/retry fields when given", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.send("tick", { event: "progress", id: "42", retry: 3000 });

    expect(res.write).toHaveBeenCalledWith("id: 42\nevent: progress\nretry: 3000\ndata: tick\n\n");
  });

  it("send() splits multi-line payloads into multiple data: lines", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.send("line1\nline2");

    expect(res.write).toHaveBeenCalledWith("data: line1\ndata: line2\n\n");
  });

  it("comment() writes an SSE comment line", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.comment("ping");

    expect(res.write).toHaveBeenCalledWith(": ping\n\n");
  });

  it("close() ends the response and is idempotent", () => {
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(mockReq()));
    stream.close();
    stream.close();

    expect(res.end).toHaveBeenCalledTimes(1);
  });

  it("stops writing after the client disconnects", () => {
    const req = mockReq();
    const res = mockRes();
    const stream = new SseStream(res, disconnectSignal(req));
    (req as unknown as EventEmitter).emit("close");

    stream.send("late event");
    stream.comment("late ping");

    expect(res.write).not.toHaveBeenCalled();
  });

  it("exposes the signal passed at construction", () => {
    const req = mockReq();
    const res = mockRes();
    const signal = disconnectSignal(req);
    const stream = new SseStream(res, signal);

    expect(stream.signal).toBe(signal);
  });
});
