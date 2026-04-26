import { describe, expect, it } from "vitest";

import { Token } from "./token.js";

describe("Token", () => {
  it("retains its description", () => {
    const t = new Token<string>("LOGGER");
    expect(t.description).toBe("LOGGER");
  });

  it("renders identifiably in toString", () => {
    const t = new Token<number>("MAX_RETRIES");
    expect(`${t}`).toBe("Token<MAX_RETRIES>");
  });

  it("two tokens with the same description are not equal by identity", () => {
    const a = new Token<string>("X");
    const b = new Token<string>("X");
    expect(a).not.toBe(b);
  });
});
