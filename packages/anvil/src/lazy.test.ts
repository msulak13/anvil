import { describe, expect, it, vi } from "vitest";

import { Lazy } from "./lazy.js";

describe("Lazy", () => {
  it("does not call the factory until get()", () => {
    const factory = vi.fn(() => 1);
    new Lazy(factory);
    expect(factory).not.toHaveBeenCalled();
  });

  it("memoizes the first result", () => {
    const factory = vi.fn(() => ({}));
    const lazy = new Lazy(factory);
    expect(lazy.get()).toBe(lazy.get());
    expect(factory).toHaveBeenCalledTimes(1);
  });

  it("memoizes an undefined result", () => {
    const factory = vi.fn(() => undefined);
    const lazy = new Lazy(factory);
    lazy.get();
    lazy.get();
    expect(factory).toHaveBeenCalledTimes(1);
  });

  it("retries after the factory throws", () => {
    let calls = 0;
    const lazy = new Lazy(() => {
      if (++calls === 1) throw new Error("first");
      return calls;
    });
    expect(() => lazy.get()).toThrow("first");
    expect(lazy.get()).toBe(2);
  });
});
