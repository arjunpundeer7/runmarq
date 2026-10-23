import type { Preferences, RunView, StreamMessage } from "@runmarq/contracts";
import { create } from "zustand";
import type { AppInfo, Route } from "../services/native";
import { errorMessage, native } from "../services/native";

const TERMINAL = new Set(["completed", "failed", "cancelled"]);

interface AppState {
  ready: boolean;
  fatal: string | null;
  connected: boolean;
  connectionError: string | null;
  info: AppInfo | null;
  preferences: Preferences | null;
  /** Open runs keyed by id. */
  active: Record<string, RunView>;
  /** Increments when a run finishes, so history views know to refresh. */
  finishedTick: number;
  route: Route;
  back: Route[];
  toast: { id: number; text: string; tone: "info" | "error" } | null;

  init(): Promise<void>;
  apply(m: StreamMessage): void;
  navigate(r: Route): void;
  goBack(): void;
  savePreferences(p: Partial<Preferences>): Promise<void>;
  refreshInfo(): Promise<void>;
  notify(text: string, tone?: "info" | "error"): void;
}

let toastId = 0;

export const useApp = create<AppState>((set, get) => ({
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

  async init() {
    try {
      const api = await native();
      await api.onStream((m) => get().apply(m));
      await api.onConnection((c) => set({ connected: c.connected, connectionError: c.error }));
      await api.onNavigate((r) => get().navigate(r));
      const info = await api.appInfo();
      set({ info, connected: info.collector.running, connectionError: info.collector.error });
      const [preferences, runs] = await Promise.all([api.getPreferences(), api.activeRuns()]);
      set({ preferences, active: Object.fromEntries(runs.map((r) => [r.id, r])), ready: true });
    } catch (e) {
      // The UI still renders: pages show the disconnected state and settings stay reachable.
      set({ ready: true, fatal: get().info ? null : errorMessage(e), connected: false, connectionError: errorMessage(e) });
    }
  },

  apply(m) {
    switch (m.kind) {
      case "snapshot":
        set({ active: Object.fromEntries(m.runs.map((r) => [r.id, r])), connected: true });
        break;
      case "run_updated": {
        const active = { ...get().active };
        if (TERMINAL.has(m.run.state)) {
          delete active[m.run.id];
          set({ active, finishedTick: get().finishedTick + 1 });
        } else {
          active[m.run.id] = m.run;
          set({ active });
        }
        break;
      }
      case "preferences_changed":
        set({ preferences: m.preferences });
        break;
      case "notification":
        // Native notifications are delivered by the native side.
        break;
    }
  },

  navigate(r) {
    const cur = get().route;
    if (JSON.stringify(cur) === JSON.stringify(r)) return;
    set({ route: r, back: r.page === "run" ? [...get().back, cur].slice(-20) : [] });
  },

  goBack() {
    const back = [...get().back];
    const prev = back.pop() ?? { page: "overview" };
    set({ route: prev, back });
  },

  async savePreferences(partial) {
    const current = get().preferences;
    if (!current) return;
    const next = { ...current, ...partial };
    set({ preferences: next });
    try {
      const saved = await (await native()).setPreferences(next);
      set({ preferences: saved });
    } catch (e) {
      set({ preferences: current });
      get().notify(`Could not save settings: ${errorMessage(e)}`, "error");
    }
  },

  async refreshInfo() {
    try {
      const info = await (await native()).appInfo();
      set({ info, connected: info.collector.running, connectionError: info.collector.error });
    } catch (e) {
      set({ connectionError: errorMessage(e) });
    }
  },

  notify(text, tone = "info") {
    const id = ++toastId;
    set({ toast: { id, text, tone } });
    setTimeout(() => {
      if (get().toast?.id === id) set({ toast: null });
    }, 4200);
  },
}));

export const selectActiveRuns = (s: AppState): RunView[] =>
  Object.values(s.active).sort((a, b) => Date.parse(a.startedAt) - Date.parse(b.startedAt));
