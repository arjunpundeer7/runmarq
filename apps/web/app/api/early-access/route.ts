import { NextResponse } from "next/server";
import { RateLimiter, validateSignup } from "@/lib/early-access";

const limiter = new RateLimiter(5, 10 * 60_000);

export async function POST(req: Request) {
  const endpoint = process.env.EARLY_ACCESS_WEBHOOK_URL;
  if (!endpoint) {
    return NextResponse.json({ status: "not_configured", error: "Early access sign-up is not open yet." }, { status: 503 });
  }
  if (!/^https:\/\//.test(endpoint) && !/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?\//.test(endpoint)) {
    console.error("EARLY_ACCESS_WEBHOOK_URL must use https");
    return NextResponse.json({ status: "not_configured", error: "Early access sign-up is not available." }, { status: 503 });
  }
  if (!req.headers.get("content-type")?.includes("application/json")) {
    return NextResponse.json({ error: "Expected JSON." }, { status: 415 });
  }
  const ip = req.headers.get("x-forwarded-for")?.split(",")[0]?.trim() || req.headers.get("x-real-ip") || "unknown";
  if (!limiter.allow(ip)) {
    return NextResponse.json({ error: "Too many attempts. Please try again in a few minutes." }, { status: 429 });
  }

  let body: unknown;
  try {
    const text = await req.text();
    if (text.length > 2000) return NextResponse.json({ error: "Request too large." }, { status: 413 });
    body = JSON.parse(text);
  } catch {
    return NextResponse.json({ error: "Invalid request." }, { status: 400 });
  }
  const v = validateSignup(body);
  if (!v.ok) return NextResponse.json({ error: v.error }, { status: 400 });

  try {
    const res = await fetch(endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        ...(process.env.EARLY_ACCESS_WEBHOOK_SECRET ? { authorization: `Bearer ${process.env.EARLY_ACCESS_WEBHOOK_SECRET}` } : {}),
      },
      body: JSON.stringify({ ...v.value, consentedAt: new Date().toISOString(), source: "website" }),
      signal: AbortSignal.timeout(8000),
    });
    if (res.status === 409) return NextResponse.json({ status: "already_registered" });
    if (!res.ok) throw new Error(`backend returned ${res.status}`);
    return NextResponse.json({ status: "registered" });
  } catch (e) {
    console.error("early-access backend error", e instanceof Error ? e.message : e);
    return NextResponse.json({ error: "We could not save your address right now. Please try again later." }, { status: 502 });
  }
}
