import { describe, expect, it } from "vitest";
import { RateLimiter, validateSignup } from "./early-access";

describe("validateSignup", () => {
  it("accepts a valid address with consent", () => {
    expect(validateSignup({ email: " Dev@Example.com ", consent: true, platform: "linux" })).toEqual({
      ok: true,
      value: { email: "dev@example.com", platform: "linux" },
    });
  });
  it("requires consent", () => {
    expect(validateSignup({ email: "dev@example.com", consent: false }).ok).toBe(false);
  });
  it("rejects malformed and oversized addresses", () => {
    expect(validateSignup({ email: "not-an-email", consent: true }).ok).toBe(false);
    expect(validateSignup({ email: `${"a".repeat(250)}@x.io`, consent: true }).ok).toBe(false);
    expect(validateSignup(null).ok).toBe(false);
  });
  it("drops unknown platforms and rejects the honeypot", () => {
    const r = validateSignup({ email: "a@b.co", consent: true, platform: "amiga" });
    expect(r.ok && r.value.platform).toBe(null);
    expect(validateSignup({ email: "a@b.co", consent: true, company: "spam inc" }).ok).toBe(false);
  });
});

describe("RateLimiter", () => {
  it("limits per key within a window and resets after it", () => {
    const l = new RateLimiter(2, 1000);
    expect(l.allow("ip", 0)).toBe(true);
    expect(l.allow("ip", 10)).toBe(true);
    expect(l.allow("ip", 20)).toBe(false);
    expect(l.allow("other", 20)).toBe(true);
    expect(l.allow("ip", 1001)).toBe(true);
  });
});
