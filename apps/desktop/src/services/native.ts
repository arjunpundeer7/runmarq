// Typed bridge to the Tauri native side. The webview never talks to the
// collector directly and never sees its token; every call goes through a
// native command, which talks to the collector over its authenticated API.

import type {
  AgentSource,
  Diagnostics,
  HistoryPage,
  HistoryQuery,
  Insights,
  IntegrationStatus,
  Preferences,
  RepositoryView,
  RunDetail,
  RunView,
  StreamMessage,
  UsageReport,
} from "@runmarq/contracts";

export interface CollectorInfo {
  running: boolean;
  pid: number | null;
  port: number | null;
  version: string | null;
  error: string | null;
}

export interface AppInfo {
  version: string;
  platform: string;
  dataDir: string;
  logsDir: string;
  hookExecutable: string;
  collector: CollectorInfo;
}

export interface InstallPlan {
  agent: AgentSource;
  configPath: string;
  fileExists: boolean;
  alreadyInstalled: boolean;
  events: string[];
  command: string;
  preview: unknown;
  notes: string[];
}

export interface InstallOutcome {
  result: { configPath: string; backupPath: string | null; changed: boolean; pendingSteps: string[] };
  status: IntegrationStatus;
}

export interface UninstallOutcome {
  configPath: string;
  backupPath: string | null;
  removedEntries: number;
}

export interface ConnectionEvent {
  connected: boolean;
  error: string | null;
}

export type Route =
  | { page: "overview" }
  | { page: "history" }
  | { page: "integrations" }
  | { page: "insights" }
  | { page: "usage" }
  | { page: "settings" }
  | { page: "run"; id: string };

export interface NativeApi {
  appInfo(): Promise<AppInfo>;
  activeRuns(): Promise<RunView[]>;
  history(query: HistoryQuery): Promise<HistoryPage>;
  runDetail(id: string): Promise<RunDetail | null>;
  repositories(): Promise<RepositoryView[]>;
  insights(): Promise<Insights>;
  usage(): Promise<UsageReport>;
  diagnostics(): Promise<Diagnostics>;
  getPreferences(): Promise<Preferences>;
  setPreferences(p: Preferences): Promise<Preferences>;
  integrations(projectRoot: string | null): Promise<IntegrationStatus[]>;
  integrationPlan(agent: AgentSource, projectRoot: string | null): Promise<InstallPlan>;
  integrationInstall(agent: AgentSource, projectRoot: string | null): Promise<InstallOutcome>;
  integrationRemove(agent: AgentSource, projectRoot: string | null): Promise<UninstallOutcome>;
  exportCsv(): Promise<string | null>;
  deleteHistory(before: string | null): Promise<number>;
  restartCollector(): Promise<CollectorInfo>;
  openPath(kind: "data" | "logs"): Promise<void>;
  pickDirectory(): Promise<string | null>;
  setWidgetVisible(visible: boolean): Promise<void>;
  getAutostart(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<boolean>;
  getTerminalCommand(): Promise<boolean>;
  /** Returns whether new terminals will find `runmarq` without further steps. */
  setTerminalCommand(enabled: boolean): Promise<boolean>;
  onStream(cb: (m: StreamMessage) => void): Promise<() => void>;
  onConnection(cb: (c: ConnectionEvent) => void): Promise<() => void>;
  onNavigate(cb: (r: Route) => void): Promise<() => void>;
}

export const isTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriApi(): Promise<NativeApi> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const auto = await import("@tauri-apps/plugin-autostart");
  return {
    appInfo: () => invoke("app_info"),
    activeRuns: () => invoke("active_runs"),
    history: (query) => invoke("history", { query }),
    runDetail: (id) => invoke("run_detail", { id }),
    repositories: () => invoke("repositories"),
    insights: () => invoke("insights"),
    usage: () => invoke("usage"),
    diagnostics: () => invoke("diagnostics"),
    getPreferences: () => invoke("get_preferences"),
    setPreferences: (preferences) => invoke("set_preferences", { preferences }),
    integrations: (projectRoot) => invoke("integrations", { projectRoot }),
    integrationPlan: (agent, projectRoot) => invoke("integration_plan", { agent, projectRoot }),
    integrationInstall: (agent, projectRoot) => invoke("integration_install", { agent, projectRoot }),
    integrationRemove: (agent, projectRoot) => invoke("integration_remove", { agent, projectRoot }),
    exportCsv: () => invoke("export_csv"),
    deleteHistory: (before) => invoke("delete_history", { before }),
    restartCollector: () => invoke("restart_collector"),
    openPath: (kind) => invoke("open_path", { kind }),
    pickDirectory: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const r = await open({ directory: true, multiple: false, title: "Choose a project folder" });
      return typeof r === "string" ? r : null;
    },
    setWidgetVisible: (visible) => invoke("set_widget_visible", { visible }),
    getAutostart: () => auto.isEnabled(),
    setAutostart: async (enabled) => {
      if (enabled) await auto.enable();
      else await auto.disable();
      return auto.isEnabled();
    },
    getTerminalCommand: () => invoke("terminal_command"),
    setTerminalCommand: (enabled) => invoke("set_terminal_command", { enabled }),
    onStream: (cb) => listen<StreamMessage>("runmarq://stream", (e) => cb(e.payload)),
    onConnection: (cb) => listen<ConnectionEvent>("runmarq://connection", (e) => cb(e.payload)),
    onNavigate: (cb) => listen<Route>("runmarq://navigate", (e) => cb(e.payload)),
  };
}

let api: Promise<NativeApi> | null = null;
let override: NativeApi | null = null;

/** Tests inject a fake implementation. */
export function setNativeApi(impl: NativeApi | null): void {
  override = impl;
  api = null;
}

export function native(): Promise<NativeApi> {
  if (override) return Promise.resolve(override);
  if (!api) {
    api = isTauri() ? tauriApi() : Promise.reject(new Error("runmarq's interface runs inside the desktop app."));
  }
  return api;
}

/** Converts unknown errors (Tauri returns strings) into readable messages. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong.";
}
