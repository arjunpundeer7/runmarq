// @vitest-environment jsdom
import type { RunView } from "@runmarq/contracts";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { RunCardData } from "./RunCard";
import { RunCard } from "./RunCard";

afterEach(cleanup);

const run: RunCardData & Pick<RunView, "attentionDetail"> = {
  id: "run_1",
  agent: "claude_code",
  repositoryName: "acme-web",
  title: "Fix the login test",
  taskCategory: "bug_fix",
  promptKnown: true,
  state: "running",
  startedAt: "2026-01-01T10:00:00Z",
  endedAt: null,
  wallMs: 600_000,
  activeMs: 600_000,
  currentActivity: "running_tests",
  attention: "stuck",
  attentionDetail: "The same command failed 3 times in a row",
  plan: null,
  forecast: null,
};

describe("RunCard attention", () => {
  it("says why a run looks stuck", () => {
    render(<RunCard run={run} now={Date.parse("2026-01-01T10:10:00Z")} />);
    expect(screen.getByText("Claude Code may be stuck. The same command failed 3 times in a row.")).toBeTruthy();
  });

  it("still flags a stuck run without a reason", () => {
    const bare: typeof run = { ...run, attentionDetail: undefined };
    render(<RunCard run={bare} now={Date.parse("2026-01-01T10:10:00Z")} />);
    expect(screen.getByText("Claude Code may be stuck.")).toBeTruthy();
  });
});
