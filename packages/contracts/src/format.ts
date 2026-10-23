// Display helpers shared by the desktop app and the website. Wording and
// rounding mirror the Rust implementations (crates/core/src/forecast.rs) so
// the CLI, tray and app say the same thing.

import type { ActivityCategory, AgentSource, EtaForecast, Phase, RunState, RunView, TaskCategory } from "./generated";

const unitRound = (s: number): [number, "s" | "min" | "h"] => {
  if (s < 90) return [Math.max(1, Math.round(s)), "s"];
  if (s < 7200) return [Math.max(1, Math.round(s / 60)), "min"];
  return [Math.round(s / 3600), "h"];
};

/** "About 5–12 min", "Likely under 2 min". Never false precision. */
export function humanizeRange(p10: number, p90: number): string {
  if (p90 < 60) return "Likely under a minute";
  if (p90 < 120) return "Likely under 2 min";
  const [lo, lu] = unitRound(Math.max(p10, 30));
  const [hi, hu] = unitRound(p90);
  if (lu === hu) return lo >= hi ? `About ${hi} ${hu}` : `About ${lo}–${hi} ${hu}`;
  return `About ${lo} ${lu} – ${hi} ${hu}`;
}

const QUARTILE_RATIO = 0.67449 / 1.28155;

/**
 * The likely range: the 25th to 75th percentile, read from the 10th, 50th and
 * 90th on a log scale, each side with its own spread.
 */
export function likelyRange(p10: number, p50: number, p90: number): [number, number] {
  const ln = (v: number) => Math.log(Math.max(1, v));
  const m = ln(p50);
  const lo = Math.exp(m - QUARTILE_RATIO * (m - ln(Math.min(p10, p50))));
  const hi = Math.exp(m + QUARTILE_RATIO * (ln(Math.max(p90, p50)) - m));
  return [Math.round(lo), Math.round(hi)];
}

/** The likely range as text, and "could take up to …" when the tail reaches well past it. */
export function humanizeEta(p10: number, p50: number, p90: number): { likely: string; tail: string | null } {
  const [lo, hi] = likelyRange(p10, p50, p90);
  const [tv, tu] = unitRound(p90);
  const [hv, hu] = unitRound(Math.max(hi, 30));
  const far = p90 >= 120 && p90 >= 1.5 * Math.max(hi, 30) && (tv !== hv || tu !== hu);
  return { likely: humanizeRange(lo, hi), tail: far ? `could take up to ${tv} ${tu}` : null };
}

/** Elapsed time as m:ss or h:mm:ss. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** Coarse duration for history and insights: "45 s", "6 min", "1 h 5 min". */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.round(s / 60)} min`;
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return m ? `${h} h ${m} min` : `${h} h`;
}

export const AGENT_NAMES: Record<AgentSource, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
};

export const STATE_LABELS: Record<RunState, string> = {
  created: "Starting",
  estimating: "Estimating",
  running: "Running",
  waiting_for_user: "Waiting for you",
  waiting_for_external_resource: "Waiting on external resource",
  blocked: "Blocked",
  completed: "Run finished",
  failed: "Failed",
  cancelled: "Cancelled",
  unknown: "Status unknown",
};

export const ACTIVITY_LABELS: Record<ActivityCategory, string> = {
  reading_files: "Reading files",
  editing_code: "Editing code",
  searching_repository: "Searching repository",
  running_commands: "Running commands",
  installing_dependencies: "Installing dependencies",
  building: "Building",
  running_tests: "Running tests",
  debugging: "Debugging",
  waiting_for_approval: "Waiting for approval",
  research: "Researching",
  delegating: "Delegating to subagent",
  compacting_context: "Compacting context",
  unknown: "Working",
};

export const CATEGORY_LABELS: Record<TaskCategory, string> = {
  small_edit: "Small edit",
  bug_fix: "Bug fix",
  feature_implementation: "Feature",
  ui_development: "UI work",
  refactoring: "Refactor",
  testing: "Testing",
  dependency_work: "Dependencies",
  repository_analysis: "Analysis",
  research: "Research",
  migration: "Migration",
  multi_step: "Multi-step",
  unknown: "General task",
};

export const EXIT_REASON_LABELS: Record<string, string> = {
  agent_finished: "The agent reported the end of its turn.",
  agent_error: "The agent reported an error.",
  agent_aborted: "The agent reported the turn was aborted.",
  user_interrupt: "Interrupted by the user.",
  superseded: "A new prompt arrived before this run reported an end.",
  idle_without_completion: "The agent went idle without reporting the end of this run.",
  session_ended: "The session ended while the run was open.",
  process_exited: "The agent process exited.",
  stale: "No updates for 20 minutes.",
  collector_restarted: "runmarq restarted while this run was open.",
};

/** Human text for reason codes attached to a forecast. */
export const REASON_LABELS: Record<string, string> = {
  cold_start_prior: "Based on built-in starting estimates; no local history yet",
  history_global: "Based on your overall run history",
  history_category: "Based on tasks of this type",
  history_agent_category: "Based on tasks of this type with this agent",
  history_repository: "Based on tasks of this type in this repository",
  elapsed_beyond_typical: "Running longer than similar runs",
  repeated_test_failures: "Repeated test failures",
  test_failure: "A test run failed",
  tool_failures: "Several tool calls failed",
  tests_passing: "Tests are passing",
  subagents_active: "Subagents are working",
  plan_progress: "Adjusted by plan progress",
  plan_complete: "All plan steps are done",
  finishing_up: "Wrapping up: work checks out and no new tools are running",
  no_prompt_information: "The task description was not available",
  insufficient_history: "Not enough local history",
  waiting_for_user: "Paused while waiting for you",
  status_unknown: "Status unknown",
  phase_model: "Simulated from how your runs move between phases",
  similar_tasks: "Compared with similar earlier prompts",
  learned_model: "Learned from your finished runs",
  command_history: "Uses the measured duration of the running command",
  calibrated: "Range calibrated on your recent runs",
};

export const PHASE_LABELS: Record<Phase, string> = {
  plan: "Planning",
  explore: "Exploring",
  edit: "Editing",
  verify: "Testing and building",
  execute: "Running commands",
  delegate: "Delegating",
  compact: "Compacting context",
};

/** Labels for the models named in `EtaForecast.components`. */
export const COMPONENT_LABELS: Record<string, string> = {
  history: "History baseline",
  phase: "Phase simulation",
  similar: "Similar prompts",
  learned: "Learned model",
};

/** "high" becomes "High", "xhigh" becomes "Extra high". */
export function effortLabel(effort: string): string {
  const e = effort.toLowerCase();
  if (e === "xhigh" || e === "extra_high") return "Extra high";
  return e.charAt(0).toUpperCase() + e.slice(1).replace(/_/g, " ");
}

/** Compact token counts: 950, 18.4k, 1.2M. */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0).replace(/\.0$/, "")}k`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
}

/** What a forecast means for someone deciding whether to wait. Mirrors `Outlook` in Rust. */
export type Outlook = "finishing_up" | "almost_done" | "few_minutes" | "a_while" | "long_task";

export const OUTLOOK_LABELS: Record<Outlook, string> = {
  finishing_up: "Finishing up",
  almost_done: "Almost done",
  few_minutes: "A few minutes",
  a_while: "Time for something else",
  long_task: "Long task",
};

export function outlookOf(p10: number, p50: number, p90: number): Outlook {
  const [, hi] = likelyRange(p10, p50, p90);
  if (hi <= 120) return "almost_done";
  if (hi <= 600) return "few_minutes";
  if (p50 <= 1800) return "a_while";
  return "long_task";
}

export interface EtaText {
  /** Primary line, for example "Time for something else" or "About 8–14 min". */
  headline: string;
  /** The long tail, for example "could take up to 45 min", when it matters. */
  tail?: string | null;
  /** Secondary line: the likely range under an outlook, the outlook under a range, or "paused". */
  detail: string;
  kind: "estimate" | "waiting" | "unknown" | "insufficient" | "estimating" | "ended";
  outlook?: Outlook;
}

export function etaText(run: Pick<RunView, "state" | "forecast">): EtaText {
  switch (run.state) {
    case "waiting_for_user":
      return { headline: "Paused", detail: "Resumes after you respond", kind: "waiting" };
    case "waiting_for_external_resource":
      return { headline: "Paused", detail: "Waiting on an external resource", kind: "waiting" };
    case "unknown":
      return { headline: "Status unknown", detail: "No recent updates from the agent", kind: "unknown" };
    case "completed":
    case "failed":
    case "cancelled":
      return { headline: STATE_LABELS[run.state], detail: "", kind: "ended" };
    default:
      break;
  }
  const f: EtaForecast | null = run.forecast;
  if (!f) return { headline: "Estimating", detail: "", kind: "estimating" };
  if (f.status === "insufficient_data") {
    return { headline: "Not enough information yet", detail: "Estimate appears as the run progresses", kind: "insufficient" };
  }
  if (f.status === "available" && f.remainingP10Seconds != null && f.remainingP50Seconds != null && f.remainingP90Seconds != null) {
    const { likely, tail } = humanizeEta(f.remainingP10Seconds, f.remainingP50Seconds, f.remainingP90Seconds);
    const outlook: Outlook = f.reasonCodes.includes("finishing_up")
      ? "finishing_up"
      : outlookOf(f.remainingP10Seconds, f.remainingP50Seconds, f.remainingP90Seconds);
    // Low confidence leads with the outlook; medium or high leads with the range.
    const ranged = f.confidence !== "low" && outlook !== "finishing_up" && outlook !== "almost_done";
    return ranged
      ? { headline: likely, detail: OUTLOOK_LABELS[outlook], tail, kind: "estimate", outlook }
      : { headline: OUTLOOK_LABELS[outlook], detail: likely, tail, kind: "estimate", outlook };
  }
  return { headline: "Estimating", detail: "", kind: "estimating" };
}

export const CONFIDENCE_LABELS = { low: "Low", medium: "Medium", high: "High" } as const;

/** Wall-clock elapsed for a live run, using the server's figures as the base. */
export function liveElapsedMs(run: Pick<RunView, "startedAt" | "endedAt" | "wallMs">, now: number = Date.now()): number {
  if (run.endedAt) return run.wallMs;
  return Math.max(run.wallMs, now - Date.parse(run.startedAt));
}
