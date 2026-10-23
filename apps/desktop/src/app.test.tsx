import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { createMockNative, forecast, run } from "./services/mock";
import { setNativeApi } from "./services/native";
import { useApp } from "./stores/app";

function reset() {
  useApp.setState({
    ready: false,
    fatal: null,
    connected: false,
    connectionError: null,
    info: null,
    preferences: null,
    active: {},
    finishedTick: 0,
    route: { page: "overview" },
    back: [],
    toast: null,
  });
}

beforeEach(reset);
afterEach(() => {
  cleanup();
  setNativeApi(null);
});

describe("Overview", () => {
  it("shows active runs with ranged estimates", async () => {
    setNativeApi(createMockNative());
    render(<App />);
    expect(await screen.findByText("Implement authentication")).toBeTruthy();
    expect(screen.getByText("About 9–12 min")).toBeTruthy();
    expect(screen.getByText("Codex is waiting for your approval.")).toBeTruthy();
    expect(screen.getByText("1 agent working, 1 waiting for you.")).toBeTruthy();
  });

  it("explains the empty state", async () => {
    setNativeApi(createMockNative({ active: [] }));
    render(<App />);
    expect(await screen.findByText("Your agent runs will appear here.")).toBeTruthy();
  });

  it("shows an actionable banner when the collector is offline", async () => {
    setNativeApi(createMockNative({ connected: false, active: [] }));
    render(<App />);
    expect(await screen.findByText("The collector is not reachable.")).toBeTruthy();
  });

  it("applies live stream updates and removes finished runs", async () => {
    const mock = createMockNative({ active: [] });
    setNativeApi(mock);
    render(<App />);
    await screen.findByText("Your agent runs will appear here.");
    mock.emit({
      kind: "run_updated",
      run: run({ id: "live", title: "Live run", forecast: forecast("live", 200, 400, 900) }),
    });
    expect(await screen.findByText("Live run")).toBeTruthy();
    expect(screen.getByText("About 5–10 min")).toBeTruthy();
    mock.emit({ kind: "run_updated", run: run({ id: "live", title: "Live run", state: "completed", endedAt: new Date().toISOString() }) });
    await waitFor(() => expect(screen.queryByText("About 5–10 min")).toBeNull());
  });
});

describe("Navigation", () => {
  it("moves between primary sections", async () => {
    setNativeApi(createMockNative());
    render(<App />);
    await screen.findByText("Implement authentication");
    fireEvent.click(screen.getByRole("button", { name: "History" }));
    expect(await screen.findByText("Fix the failing login test")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Insights" }));
    expect(await screen.findByText("Not enough finished runs to measure accuracy yet.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Integrations" }));
    expect(await screen.findByText("Waiting for first event")).toBeTruthy();
  });

  it("opens run details from the overview", async () => {
    setNativeApi(createMockNative());
    render(<App />);
    await screen.findByText("Implement authentication");
    fireEvent.click(screen.getAllByRole("button", { name: /Details/ })[0]!);
    expect(await screen.findByText("Forecast history")).toBeTruthy();
    expect(screen.getAllByText("Estimate increased after a test failure.").length).toBeGreaterThan(0);
    expect(screen.getByText("Run activity")).toBeTruthy();
    expect(screen.getByText("18k (6.1k thinking)")).toBeTruthy();
    expect(screen.getByText("4 files, 141 lines changed")).toBeTruthy();
    expect(screen.getByText("Testing and building")).toBeTruthy();
    expect(screen.getByText(/Models used: History baseline, Phase simulation/)).toBeTruthy();
  });

  it("explains which estimation models are in use", async () => {
    setNativeApi(createMockNative());
    useApp.setState({ route: { page: "insights" } });
    render(<App />);
    expect(await screen.findByText("Estimation model")).toBeTruthy();
    expect(screen.getByText("Trains after 100 finished runs; has 5.")).toBeTruthy();
    expect(screen.getAllByText("Not in use")).toHaveLength(3);
    expect(screen.getByText(/Range calibration starts after 34 finished runs/)).toBeTruthy();
  });
});

describe("Integrations", () => {
  it("never shows connected for a configured integration without events", async () => {
    setNativeApi(createMockNative());
    useApp.setState({ route: { page: "integrations" } });
    render(<App />);
    const codex = await screen.findByText("Waiting for first event");
    expect(codex).toBeTruthy();
    expect(screen.getAllByText("Connected")).toHaveLength(1);
  });

  it("shows the exact change before connecting", async () => {
    setNativeApi(createMockNative());
    useApp.setState({ route: { page: "integrations" } });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Set up" }));
    expect(await screen.findByText(/emit --source claude_code/, { selector: "dd" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Approve and connect" })).toBeTruthy();
  });
});

describe("Settings", () => {
  it("persists preference changes through the native bridge", async () => {
    const mock = createMockNative();
    const spy = vi.spyOn(mock, "setPreferences");
    setNativeApi(mock);
    useApp.setState({ route: { page: "settings" } });
    render(<App />);
    fireEvent.click(await screen.findByRole("switch", { name: "Quiet mode" }));
    await waitFor(() => expect(spy).toHaveBeenCalledWith(expect.objectContaining({ quietMode: true })));
  });

  it("lets each estimation signal be turned off", async () => {
    const mock = createMockNative();
    const spy = vi.spyOn(mock, "setPreferences");
    setNativeApi(mock);
    useApp.setState({ route: { page: "settings" } });
    render(<App />);
    fireEvent.click(await screen.findByRole("switch", { name: "Compare prompts with earlier ones" }));
    await waitFor(() => expect(spy).toHaveBeenCalledWith(expect.objectContaining({ similarPrompts: false })));
    expect(screen.getByRole("switch", { name: "Read token counts from agent transcripts" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Measure working-tree changes" })).toBeTruthy();
  });
});

describe("Onboarding", () => {
  it("walks through detection to the ready state", async () => {
    const mock = createMockNative({ preferences: { onboardingCompleted: false } });
    setNativeApi(mock);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
    expect(await screen.findByText("Agents on this computer")).toBeTruthy();
    expect(screen.getByText("Not detected")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Connect your agents")).toBeTruthy();
  });
});
