import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import { createMockNative, sampleUsage } from "../services/mock";
import { setNativeApi } from "../services/native";
import { useApp } from "../stores/app";

beforeEach(() => {
  useApp.setState({
    ready: false,
    fatal: null,
    connected: false,
    connectionError: null,
    info: null,
    preferences: null,
    active: {},
    finishedTick: 0,
    route: { page: "usage" },
    back: [],
    toast: null,
  });
});

afterEach(() => {
  cleanup();
  setNativeApi(null);
});

describe("Usage", () => {
  it("shows the open window, today and the last 7 days per agent", async () => {
    setNativeApi(createMockNative());
    render(<App />);
    expect(await screen.findByText("126k")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Usage" })).toBeTruthy();
    expect(screen.getByText("184k")).toBeTruthy();
    expect(screen.getByText("1.2M")).toBeTruthy();
    expect(screen.getAllByText(/^Started .*, resets about /)).toHaveLength(1);
    expect(screen.getAllByText("None open")).toHaveLength(2);
    expect(screen.getByText(/Cursor does not write token counts/)).toBeTruthy();
    expect(screen.getByText(/not your plan's usage meter/)).toBeTruthy();
    expect(screen.getAllByText("Earlier windows").length).toBeGreaterThan(0);
  });

  it("never shows a limit the user did not set", async () => {
    setNativeApi(createMockNative());
    render(<App />);
    await screen.findByText("126k");
    expect(screen.queryByRole("meter")).toBeNull();
    expect(screen.queryByText(/budget \(/)).toBeNull();
  });

  it("shows progress against the user's own budget", async () => {
    setNativeApi(createMockNative({ preferences: { usageWindowBudgets: { claude_code: 200_000 } } }));
    render(<App />);
    expect(await screen.findByText("126k of your 200k budget (63%)")).toBeTruthy();
    expect(screen.getAllByRole("meter")).toHaveLength(1);
  });

  it("saves a budget through the native bridge", async () => {
    const mock = createMockNative();
    const spy = vi.spyOn(mock, "setPreferences");
    setNativeApi(mock);
    render(<App />);
    const [claude] = await screen.findAllByLabelText("Your budget per window");
    fireEvent.change(claude!, { target: { value: "150,000" } });
    fireEvent.click(screen.getAllByRole("button", { name: "Set" })[0]!);
    await waitFor(() =>
      expect(spy).toHaveBeenCalledWith(expect.objectContaining({ usageWindowBudgets: { claude_code: 150_000 } })),
    );
  });

  it("explains when transcript reading is off", async () => {
    setNativeApi(createMockNative({ usage: { ...sampleUsage(), transcriptsEnabled: false } }));
    render(<App />);
    expect(await screen.findByText(/no new token counts are recorded/)).toBeTruthy();
  });

  it("is reachable from the sidebar", async () => {
    setNativeApi(createMockNative());
    useApp.setState({ route: { page: "overview" } });
    render(<App />);
    await screen.findByText("Implement authentication");
    fireEvent.click(screen.getByRole("button", { name: "Usage" }));
    expect(await screen.findByText("126k")).toBeTruthy();
  });
});
