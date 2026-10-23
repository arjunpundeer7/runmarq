// Development and test double for the native bridge. Loaded only in dev
// builds with `?mock` in the URL, or by tests. Data here is sample data.

import type {
  AccuracyReport,
  EtaForecast,
  IntegrationStatus,
  ModelStatus,
  Preferences,
  RunDetail,
  RunView,
  StreamMessage,
  UsageReport,
  UsageTotals,
} from "@runmarq/contracts";
import type { InstallPlan, NativeApi } from "./native";

const now = () => Date.now();
const iso = (msAgo: number) => new Date(now() - msAgo).toISOString();

export function forecast(runId: string, p10: number, p50: number, p90: number, extra: Partial<EtaForecast> = {}): EtaForecast {
  return {
    runId,
    generatedAt: iso(0),
    remainingP10Seconds: p10,
    remainingP50Seconds: p50,
    remainingP90Seconds: p90,
    expectedFinishAt: new Date(now() + p50 * 1000).toISOString(),
    confidence: "medium",
    status: "available",
    reasonCodes: ["history_agent_category", "phase_model"],
    modelVersion: "ensemble-2",
    basis: "history",
    sampleSize: 24,
    activeElapsedSeconds: 272,
    rawP10Seconds: p10,
    rawP50Seconds: p50,
    rawP90Seconds: p90,
    calibrated: false,
    components: ["history", "phase"],
    ...extra,
  };
}

export function run(partial: Partial<RunView> & { id: string }): RunView {
  return {
    sessionId: "ses_1",
    agent: "claude_code",
    repositoryId: "repo_a",
    repositoryName: "acme-web",
    title: null,
    taskCategory: "feature_implementation",
    promptKnown: true,
    model: null,
    state: "running",
    exitReason: null,
    startedAt: iso(272_000),
    endedAt: null,
    lastEventAt: iso(2_000),
    wallMs: 272_000,
    activeMs: 272_000,
    waitingMs: 0,
    currentActivity: "running_tests",
    currentTool: "Bash",
    attention: null,
    toolCalls: 14,
    toolFailures: 1,
    plan: null,
    forecast: null,
    initialP10Seconds: null,
    initialP50Seconds: null,
    initialP90Seconds: null,
    effort: null,
    phase: null,
    signals: null,
    ...partial,
  };
}

export const sampleActive: RunView[] = [
  run({
    id: "run_a",
    title: "Implement authentication",
    effort: "high",
    phase: "verify",
    forecast: forecast("run_a", 480, 660, 840),
    signals: {
      turns: 9,
      subagentTurns: 0,
      outputTokens: 18_400,
      thinkingTokens: 6_100,
      contextTokens: 61_000,
      contextWindow: 200_000,
      compactions: 0,
      editLines: 132,
      diffFiles: 4,
      diffLines: 141,
      lastTurnAt: iso(4_000),
    },
  }),
  run({
    id: "run_b",
    agent: "codex",
    repositoryName: "billing-api",
    title: "Add a --json flag to the status command",
    taskCategory: "testing",
    state: "waiting_for_user",
    attention: "approval_required",
    startedAt: iso(95_000),
    wallMs: 95_000,
    activeMs: 80_000,
    waitingMs: 15_000,
    currentActivity: "running_commands",
    forecast: forecast("run_b", 0, 0, 0, { status: "waiting", remainingP10Seconds: null, remainingP50Seconds: null, remainingP90Seconds: null }),
  }),
];

const finishedRuns: RunView[] = [
  ["run_c", "completed", "Fix the failing login test", 6 * 60_000, 240, 420, 900],
  ["run_d", "completed", "Rename the Store struct", 50_000, 20, 45, 120],
  ["run_e", "cancelled", "Refactor the storage layer", 3 * 60_000, 300, 600, 1500],
  ["run_f", "failed", "Upgrade dependencies", 4 * 60_000, 120, 240, 600],
  ["run_g", "completed", "Explain the auth middleware", 70_000, 30, 80, 200],
].map(([id, state, title, wall, p10, p50, p90], i) =>
  run({
    id: id as string,
    state: state as RunView["state"],
    title: title as string,
    agent: i % 3 === 1 ? "cursor" : "claude_code",
    startedAt: iso((i + 1) * 3_600_000),
    endedAt: iso((i + 1) * 3_600_000 - (wall as number)),
    wallMs: wall as number,
    activeMs: wall as number,
    currentActivity: null,
    exitReason: state === "completed" ? "agent_finished" : state === "failed" ? "agent_error" : "superseded",
    initialP10Seconds: p10 as number,
    initialP50Seconds: p50 as number,
    initialP90Seconds: p90 as number,
  }),
);

const integrationStatuses: IntegrationStatus[] = [
  {
    agent: "claude_code",
    displayName: "Claude Code",
    detected: true,
    installedVersion: "2.1.143",
    executable: "C:\\Users\\dev\\AppData\\Roaming\\npm\\claude",
    state: "connected",
    support: "available",
    capabilities: {
      promptStart: true,
      turnCompletion: true,
      toolStarted: true,
      toolCompleted: true,
      planUpdates: true,
      approvalEvents: true,
      errorEvents: true,
      subagents: true,
      modelMetadata: true,
      transcriptUsage: true,
      interruptEvents: false,
    },
    limitations: ["Claude Code does not report user interrupts directly."],
    configLocations: ["C:\\Users\\dev\\.claude\\settings.json"],
    checks: [
      { label: "Claude Code installed", ok: true, detail: "version 2.1.143" },
      { label: "runmarq hooks registered", ok: true, detail: "13 events" },
      { label: "Event received since setup", ok: true, detail: null },
    ],
    pendingSteps: [],
    lastEventAt: iso(2000),
  },
  {
    agent: "codex",
    displayName: "Codex",
    detected: true,
    installedVersion: "0.130.0",
    executable: null,
    state: "awaiting_first_event",
    support: "limited",
    capabilities: {
      promptStart: true,
      turnCompletion: true,
      toolStarted: true,
      toolCompleted: true,
      planUpdates: true,
      approvalEvents: true,
      errorEvents: false,
      subagents: false,
      modelMetadata: true,
      transcriptUsage: true,
      interruptEvents: false,
    },
    limitations: ["Requires Codex 0.130 or later, and the runmarq hooks must be trusted once with /hooks inside Codex."],
    configLocations: ["C:\\Users\\dev\\.codex\\hooks.json"],
    checks: [{ label: "runmarq hooks registered", ok: true, detail: "7 events" }],
    pendingSteps: ["Open Codex and run /hooks, then trust the runmarq hooks. Codex does not run hooks until they are trusted."],
    lastEventAt: null,
  },
  {
    agent: "cursor",
    displayName: "Cursor",
    detected: false,
    installedVersion: null,
    executable: null,
    state: "not_detected",
    support: "limited",
    capabilities: {
      promptStart: true,
      turnCompletion: true,
      toolStarted: false,
      toolCompleted: true,
      planUpdates: true,
      approvalEvents: false,
      errorEvents: true,
      subagents: true,
      modelMetadata: true,
      transcriptUsage: false,
      interruptEvents: true,
    },
    limitations: ["Tool start and approval events are not collected."],
    configLocations: ["C:\\Users\\dev\\.cursor\\hooks.json"],
    checks: [],
    pendingSteps: [],
    lastEventAt: null,
  },
];

const defaultPrefs: Preferences = {
  storeTaskTitles: true,
  storeRepositoryNames: true,
  notificationsEnabled: true,
  notifyOnFinish: true,
  notifyOnFailure: true,
  notifyOnAttention: true,
  notifyOnDisconnect: true,
  notifyOnStuck: true,
  notificationSound: true,
  quietMode: false,
  notifyMinDurationSeconds: 20,
  retentionDays: null,
  compactWidget: false,
  widgetAlwaysOnTop: false,
  onboardingCompleted: true,
  readTranscripts: true,
  sampleGitDiff: true,
  similarPrompts: true,
  usageWindowBudgets: {},
};

const totals = (runs: number, responses: number, outputTokens: number, thinkingTokens: number, activeMin: number): UsageTotals => ({
  runs,
  responses,
  outputTokens,
  thinkingTokens,
  activeMs: activeMin * 60_000,
});

const HOUR = 3_600_000;

export function sampleUsage(): UsageReport {
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  const window = (startedHoursAgo: number, t: UsageTotals) => ({
    startedAt: iso(startedHoursAgo * HOUR),
    endsAt: iso((startedHoursAgo - 5) * HOUR),
    totals: t,
  });
  return {
    generatedAt: iso(0),
    todayStartedAt: midnight.toISOString(),
    windowHours: 5,
    transcriptsEnabled: true,
    agents: [
      {
        agent: "claude_code",
        reportsTokens: true,
        today: totals(6, 142, 184_000, 41_000, 65),
        lastSevenDays: totals(31, 820, 1_240_000, 266_000, 360),
        currentWindow: window(2.25, totals(4, 96, 126_000, 30_500, 45)),
        recentWindows: [window(9, totals(2, 46, 58_000, 10_500, 20)), window(30, totals(7, 210, 301_000, 64_000, 95))],
        windowBudgetTokens: null,
      },
      {
        agent: "codex",
        reportsTokens: true,
        today: totals(1, 12, 9_800, 2_100, 10),
        lastSevenDays: totals(5, 61, 52_000, 11_000, 48),
        currentWindow: null,
        recentWindows: [window(26, totals(3, 40, 33_000, 7_200, 30))],
        windowBudgetTokens: null,
      },
      {
        agent: "cursor",
        reportsTokens: false,
        today: totals(0, 0, 0, 0, 0),
        lastSevenDays: totals(2, 0, 0, 0, 15),
        currentWindow: null,
        recentWindows: [],
        windowBudgetTokens: null,
      },
    ],
  };
}

const emptyAccuracy: AccuracyReport = {
  evaluatedRuns: 4,
  minimumRunsForMetrics: 10,
  medianAbsErrorSeconds: null,
  meanAbsErrorSeconds: null,
  medianAbsPctError: null,
  intervalCoverage: null,
  insufficientInfoFraction: null,
  stability: null,
  byForecastAge: [],
  byAgent: [],
  byCategory: [],
  holdout: null,
};

const sampleModel: ModelStatus = {
  version: "ensemble-2",
  trainedAt: iso(600_000),
  finishedRuns: 5,
  components: [
    { key: "history", label: "History baseline", active: true, detail: "Fitted to 5 finished runs by agent, task type, repository, effort and model." },
    { key: "phase", label: "Phase simulation", active: false, detail: "Needs 10 finished runs with tool activity; has 5." },
    { key: "similar", label: "Similar prompts", active: false, detail: "Needs finished runs with prompt sketches; none yet." },
    { key: "learned", label: "Learned model", active: false, detail: "Trains after 100 finished runs; has 5." },
  ],
  calibration: { active: false, scores: 0, validationRuns: 0, requiredRuns: 34, targetCoverage: 0.8 },
};

export interface MockOptions {
  active?: RunView[];
  history?: RunView[];
  preferences?: Partial<Preferences>;
  connected?: boolean;
  usage?: UsageReport;
}

export function createMockNative(opts: MockOptions = {}): NativeApi & { emit: (m: StreamMessage) => void } {
  let prefs: Preferences = { ...defaultPrefs, ...opts.preferences };
  const active = opts.active ?? sampleActive;
  const history = opts.history ?? finishedRuns;
  const listeners: Array<(m: StreamMessage) => void> = [];
  const connected = opts.connected ?? true;
  const ok = <T>(v: T) => (connected ? Promise.resolve(v) : Promise.reject("the runmarq collector is not running"));

  const detail = (id: string): RunDetail | null => {
    const r = [...active, ...history].find((x) => x.id === id);
    if (!r) return null;
    const start = Date.parse(r.startedAt);
    const at = (s: number) => new Date(start + s * 1000).toISOString();
    return {
      run: r,
      events: [
        { id: "e1", eventType: "prompt_submitted", sourceTimestamp: at(0), receivedTimestamp: at(0), activity: null, tool: null, durationMs: null, status: null },
        { id: "e2", eventType: "tool_completed", sourceTimestamp: at(8), receivedTimestamp: at(8), activity: "reading_files", tool: "Read", durationMs: 14, status: null },
        { id: "e3", eventType: "tool_failed", sourceTimestamp: at(60), receivedTimestamp: at(60), activity: "running_tests", tool: "Bash", durationMs: 8120, status: null },
        { id: "e4", eventType: "tool_completed", sourceTimestamp: at(90), receivedTimestamp: at(90), activity: "editing_code", tool: "Edit", durationMs: 22, status: null },
      ],
      forecasts: [0, 30, 60, 90, 150, 210].map((t, i) =>
        forecast(r.id, 200 - t * 0.5, 420 - t * 0.6 + (i === 2 ? 140 : 0), 900 - t, {
          activeElapsedSeconds: t,
          generatedAt: at(t),
          reasonCodes: i === 2 ? ["test_failure", "estimate_increased"] : ["history_agent_category"],
        }),
      ),
      phases: [
        { activity: "reading_files", startedAt: at(2), endedAt: at(50), toolCalls: 4 },
        { activity: "running_tests", startedAt: at(50), endedAt: at(80), toolCalls: 1 },
        { activity: "editing_code", startedAt: at(80), endedAt: r.endedAt, toolCalls: 3 },
      ],
      changes: [{ at: at(60), message: "Estimate increased after a test failure." }],
    };
  };

  return {
    emit: (m) => listeners.forEach((l) => l(m)),
    appInfo: () =>
      Promise.resolve({
        version: "0.1.0",
        platform: "windows-x86_64",
        dataDir: "C:\\Users\\dev\\AppData\\Local\\runmarq",
        logsDir: "C:\\Users\\dev\\AppData\\Local\\runmarq\\logs",
        hookExecutable: "C:\\Users\\dev\\AppData\\Local\\runmarq\\bin\\runmarq.exe",
        collector: connected
          ? { running: true, pid: 4242, port: 51234, version: "0.1.0", error: null }
          : { running: false, pid: null, port: null, version: null, error: "the runmarq collector is not running" },
      }),
    activeRuns: () => ok(active),
    history: (q) => {
      const rows = history.filter((r) => (!q.agent || r.agent === q.agent) && (!q.state || r.state === q.state));
      return ok({ runs: rows.slice(q.offset ?? 0, (q.offset ?? 0) + (q.limit ?? 50)), total: rows.length });
    },
    runDetail: (id) => ok(detail(id)),
    repositories: () => ok([{ id: "repo_a", displayName: "acme-web", runCount: 5 }]),
    usage: () => ok(opts.usage ?? sampleUsage()),
    insights: () =>
      ok({
        totalRuns: history.length + active.length,
        finishedRuns: history.length,
        medianDurationMs: 240_000,
        totalWaitingMs: 95_000,
        byAgent: [{ key: "claude_code", label: "Claude Code", runs: 3, medianMs: null, p90Ms: null }],
        byCategory: [],
        accuracy: emptyAccuracy,
        model: sampleModel,
      }),
    diagnostics: () =>
      ok({
        collectorVersion: "0.1.0",
        schemaVersion: 1,
        pid: 4242,
        startedAt: iso(3_600_000),
        uptimeSeconds: 3600,
        dataDir: "C:\\Users\\dev\\AppData\\Local\\runmarq",
        databaseBytes: 1_200_000,
        eventsReceived: 412,
        eventsRejected: 0,
        eventsDuplicate: 3,
        spooledEventsReplayed: 6,
        spoolPending: 0,
        subscribers: 1,
        openRuns: active.length,
        ingestLatency: { samples: 412, p50Ms: 0.4, p95Ms: 0.9, maxMs: 3.1 },
        forecastLatency: { samples: 380, p50Ms: 0.02, p95Ms: 0.05, maxMs: 0.4 },
        dbWriteLatency: { samples: 412, p50Ms: 0.2, p95Ms: 0.5, maxMs: 2.2 },
        hookLatency: { samples: 409, p50Ms: 0.3, p95Ms: 0.6, maxMs: 4 },
        trainingLatency: { samples: 3, p50Ms: 180, p95Ms: 240, maxMs: 240 },
        recentErrors: [],
      }),
    getPreferences: () => ok(prefs),
    setPreferences: (p) => {
      prefs = p;
      return ok(p);
    },
    integrations: () => Promise.resolve(integrationStatuses),
    integrationPlan: (agent) =>
      Promise.resolve<InstallPlan>({
        agent,
        configPath: "C:\\Users\\dev\\.claude\\settings.json",
        fileExists: true,
        alreadyInstalled: false,
        events: ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"],
        command: "C:/Users/dev/AppData/Local/runmarq/bin/runmarq.exe emit --source claude_code",
        preview: { hooks: { Stop: [{ hooks: [{ type: "command", command: "runmarq emit --source claude_code", timeout: 10 }] }] } },
        notes: ["Hooks print nothing and always exit 0."],
      }),
    integrationInstall: (agent) =>
      Promise.resolve({
        result: { configPath: "settings.json", backupPath: null, changed: true, pendingSteps: [] },
        status: { ...integrationStatuses.find((s) => s.agent === agent)!, state: "awaiting_first_event" },
      }),
    integrationRemove: () => Promise.resolve({ configPath: "settings.json", backupPath: null, removedEntries: 13 }),
    exportCsv: () => Promise.resolve(null),
    deleteHistory: () => Promise.resolve(history.length),
    restartCollector: () => Promise.resolve({ running: true, pid: 4243, port: 51235, version: "0.1.0", error: null }),
    openPath: () => Promise.resolve(),
    pickDirectory: () => Promise.resolve(null),
    setWidgetVisible: () => Promise.resolve(),
    getAutostart: () => Promise.resolve(false),
    setAutostart: (v) => Promise.resolve(v),
    getTerminalCommand: () => Promise.resolve(false),
    setTerminalCommand: (v) => Promise.resolve(v),
    onStream: (cb) => {
      listeners.push(cb);
      return Promise.resolve(() => listeners.splice(listeners.indexOf(cb), 1));
    },
    onConnection: () => Promise.resolve(() => {}),
    onNavigate: () => Promise.resolve(() => {}),
  };
}
