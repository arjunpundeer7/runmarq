// Early-access sign-up: validation and rate limiting, shared by the route
// handler and its tests. Storage is delegated to a configured backend
// (EARLY_ACCESS_WEBHOOK_URL); without one the endpoint reports that sign-up
// is not open instead of pretending to accept addresses.

export const PLATFORMS = ["macos", "windows", "linux"] as const;
export type Platform = (typeof PLATFORMS)[number];

export interface SignupInput {
  email: string;
  platform: Platform | null;
}

export type Validation = { ok: true; value: SignupInput } | { ok: false; error: string };

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;

export function validateSignup(body: unknown): Validation {
  if (typeof body !== "object" || body == null) return { ok: false, error: "Invalid request." };
  const b = body as Record<string, unknown>;
  // Honeypot: a field hidden from people. Bots fill it in.
  if (typeof b.company === "string" && b.company.length > 0) return { ok: false, error: "Invalid request." };
  const email = typeof b.email === "string" ? b.email.trim().toLowerCase() : "";
  if (!email || email.length > 254 || !EMAIL.test(email)) return { ok: false, error: "Enter a valid email address." };
  if (b.consent !== true) return { ok: false, error: "Please confirm that we may email you about early access." };
  const platform = typeof b.platform === "string" && (PLATFORMS as readonly string[]).includes(b.platform) ? (b.platform as Platform) : null;
  return { ok: true, value: { email, platform } };
}

/**
 * Fixed-window rate limiter keyed by client address. Per server instance;
 * a deployment with several instances should also limit at the edge.
 */
export class RateLimiter {
  private hits = new Map<string, { count: number; reset: number }>();

  constructor(
    private readonly limit: number,
    private readonly windowMs: number,
  ) {}

  allow(key: string, now = Date.now()): boolean {
    const entry = this.hits.get(key);
    if (!entry || now >= entry.reset) {
      this.hits.set(key, { count: 1, reset: now + this.windowMs });
      this.sweep(now);
      return true;
    }
    entry.count += 1;
    return entry.count <= this.limit;
  }

  private sweep(now: number) {
    if (this.hits.size < 5000) return;
    for (const [k, v] of this.hits) if (now >= v.reset) this.hits.delete(k);
  }
}
