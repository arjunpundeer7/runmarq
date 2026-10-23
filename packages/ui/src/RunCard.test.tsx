// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { RunCardData } from "./RunCard";
import { RunCard } from "./RunCard";

afterEach(cleanup);

const base: RunCardData = {
  id: "run_1",
  agent: "claude_code",
  repositoryName: "acme-web",
  title: "Implement authentication",
  taskCategory: "feature_implementation",
  promptKnown: true,
  state: "running",
  startedAt: new Date(Date.parse("2026-01-01T10:00:00Z")).toISOString(),
  endedAt: null,
  wallMs: 272_000,
  activeMs: 272_000,
  currentActivity: "running_tests",
  attention: null,
  plan: null,
  forecast: {
    runId: "run_1",
    generatedAt: "2026-01-01T10:04:32Z",
    remainingP10Seconds: 480,
    remainingP50Seconds: 600,
    remainingP90Seconds: 840,
    expectedFinishAt: null,
    confidence: "medium",
    status: "available",
    reasonCodes: [],
    modelVersion: "ensemble-2",
    basis: "history",
    sampleSize: 30,
    activeElapsedSeconds: 272,
    rawP10Seconds: 480,
    rawP50Seconds: 600,
    rawP90Seconds: 840,
    calibrated: false,
    components: ["history"],
  },
};

describe("RunCard", () => {
  it("shows the estimate as a range with confidence", () => {
    render(<RunCard run={base} now={Date.parse("2026-01-01T10:04:32Z")} />);
    expect(screen.getByText("About 9–12 min")).toBeTruthy();
    expect(screen.getByText("04:32 elapsed")).toBeTruthy();
    expect(screen.getByText("Running tests")).toBeTruthy();
    expect(screen.getByText("Medium")).toBeTruthy();
    expect(screen.getByRole("img").getAttribute("aria-label")).toContain("Likely finishes between");
  });

  it("pauses the estimate while waiting for approval", () => {
    render(<RunCard run={{ ...base, state: "waiting_for_user", attention: "approval_required" }} />);
    expect(screen.getAllByText("Waiting for you").length).toBeGreaterThan(0);
    expect(screen.getByText("Resumes after you respond")).toBeTruthy();
    expect(screen.getByText("Paused")).toBeTruthy();
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("never presents unknown state as finished", () => {
    render(<RunCard run={{ ...base, state: "unknown", forecast: null, attention: "connection_lost" }} />);
    expect(screen.getAllByText("Status unknown").length).toBeGreaterThan(0);
    expect(screen.queryByText("Run finished")).toBeNull();
  });

  it("shows plan progress instead of a fake percentage", () => {
    render(<RunCard run={{ ...base, plan: { totalSteps: 4, completedSteps: 3 } }} />);
    expect(screen.getByText("3 of 4 steps")).toBeTruthy();
  });

  it("labels runs that started without a prompt", () => {
    render(<RunCard run={{ ...base, title: null, promptKnown: false, forecast: null }} />);
    expect(screen.getByText("Run started without a prompt event")).toBeTruthy();
  });
});
