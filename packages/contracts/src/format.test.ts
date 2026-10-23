import { describe, expect, it } from "vitest";
import { effortLabel, etaText, formatClock, formatDuration, formatTokens, humanizeEta, humanizeRange, likelyRange, outlookOf } from "./format";
import type { EtaForecast } from "./generated";
import { INTEGRATIONS } from "./integrations";
import { validReleases } from "./releases";

describe("humanizeRange mirrors the Rust implementation", () => {
  it.each([
    [5, 40, "Likely under a minute"],
    [30, 100, "Likely under 2 min"],
    [300, 720, "About 5–12 min"],
    [1800, 9000, "About 30 min – 3 h"],
  ])("%i–%i", (a, b, out) => expect(humanizeRange(a, b)).toBe(out));
});

describe("formatting", () => {
  it("clock", () => {
    expect(formatClock(272_000)).toBe("04:32");
    expect(formatClock(3_725_000)).toBe("1:02:05");
  });
  it("duration", () => {
    expect(formatDuration(45_000)).toBe("45 s");
    expect(formatDuration(360_000)).toBe("6 min");
    expect(formatDuration(null)).toBe("—");
  });
});

describe("signals", () => {
  it("tokens", () => {
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(1000)).toBe("1k");
    expect(formatTokens(18_400)).toBe("18k");
    expect(formatTokens(2_450)).toBe("2.5k");
    expect(formatTokens(1_200_000)).toBe("1.2M");
  });
  it("effort", () => {
    expect(effortLabel("high")).toBe("High");
    expect(effortLabel("xhigh")).toBe("Extra high");
  });
});

describe("likely range mirrors the Rust implementation", () => {
  it("is the central half of a log-normal", () => {
    const q = (z: number) => Math.round(600 * Math.exp(z));
    const [lo, hi] = likelyRange(q(-1.28155), 600, q(1.28155));
    expect(Math.abs(lo - q(-0.67449))).toBeLessThanOrEqual(1);
    expect(Math.abs(hi - q(0.67449))).toBeLessThanOrEqual(1);
  });
  it("keeps the lower end when the tail grows", () => {
    expect(likelyRange(300, 600, 7200)[0]).toBe(likelyRange(300, 600, 900)[0]);
  });
  it("names the tail only when it reaches well past the range", () => {
    expect(humanizeEta(57, 400, 2700)).toEqual({ likely: "About 2–18 min", tail: "could take up to 45 min" });
    expect(humanizeEta(300, 420, 560).tail).toBeNull();
  });
});

describe("etaText", () => {
  it("pauses while waiting", () => {
    const t = etaText({ state: "waiting_for_user", forecast: null });
    expect(t.kind).toBe("waiting");
    expect(t.headline).toBe("Paused");
  });
  it("never claims completion for unknown", () => {
    expect(etaText({ state: "unknown", forecast: null }).headline).toBe("Status unknown");
  });

  const forecast = (q: [number, number, number], confidence: EtaForecast["confidence"], reasonCodes: string[] = []) =>
    ({
      status: "available",
      remainingP10Seconds: q[0],
      remainingP50Seconds: q[1],
      remainingP90Seconds: q[2],
      confidence,
      reasonCodes,
    }) as EtaForecast;

  it("leads with the outlook at low confidence, as the Rust side does", () => {
    const t = etaText({ state: "running", forecast: forecast([150, 700, 2000], "low") });
    expect(t.headline).toBe("Time for something else");
    expect(t.detail).toBe("About 5–20 min");
    const m = etaText({ state: "running", forecast: forecast([150, 700, 2000], "medium") });
    expect(m.headline).toBe("About 5–20 min");
    expect(m.detail).toBe("Time for something else");
  });

  it("says finishing up when the forecast carries that reason", () => {
    const t = etaText({ state: "running", forecast: forecast([20, 60, 200], "high", ["finishing_up"]) });
    expect(t.outlook).toBe("finishing_up");
    expect(t.headline).toBe("Finishing up");
  });

  it.each([
    [[10, 40, 90], "almost_done"],
    [[60, 180, 600], "few_minutes"],
    [[150, 700, 2000], "a_while"],
    [[900, 2400, 6000], "long_task"],
  ] as const)("outlook for %j is %s", (q, out) => expect(outlookOf(q[0], q[1], q[2])).toBe(out));
});

describe("manifest", () => {
  it("lists exactly the three implemented integrations", () => {
    expect(INTEGRATIONS.map((i) => i.id).sort()).toEqual(["claude_code", "codex", "cursor"]);
  });
  it("rejects releases without real checksums", () => {
    const r = validReleases({
      releases: [
        {
          version: "1.0.0",
          releaseDate: "2026-01-01",
          artifacts: [
            { platform: "windows", architecture: "x86_64", kind: "msi", artifactUrl: "https://x/y.msi", sha256: "abc", sizeBytes: 10, signed: false },
          ],
        },
      ],
    });
    expect(r).toEqual([]);
  });
});
