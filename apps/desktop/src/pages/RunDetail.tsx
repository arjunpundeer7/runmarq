import type { EventType, EventView, RunDetail as Detail, RunSignals } from "@runmarq/contracts";
import {
  ACTIVITY_LABELS,
  AGENT_NAMES,
  CATEGORY_LABELS,
  COMPONENT_LABELS,
  effortLabel,
  EXIT_REASON_LABELS,
  formatClock,
  formatDuration,
  formatTokens,
  humanizeEta,
  PHASE_LABELS,
  REASON_LABELS,
} from "@runmarq/contracts";
import { RunCard, runTitle, StateLabel } from "@runmarq/ui";
import { ChevronLeft } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";
import { Button, Notice, Section, Spinner } from "../components/controls";
import { ForecastChart } from "../components/ForecastChart";
import { useAsync } from "../hooks/useAsync";
import { useNow } from "../hooks/useNow";
import { native } from "../services/native";
import { useApp } from "../stores/app";

const EVENT_LABELS: Record<EventType, string> = {
  session_started: "Session started",
  session_ended: "Session ended",
  prompt_submitted: "Prompt submitted",
  run_started: "Turn started",
  tool_started: "Tool started",
  tool_completed: "Tool finished",
  tool_failed: "Tool failed",
  plan_updated: "Plan updated",
  subagent_started: "Subagent started",
  subagent_completed: "Subagent finished",
  approval_requested: "Approval requested",
  approval_resolved: "Approval resolved",
  agent_idle: "Agent idle",
  context_compacting: "Compacting context",
  run_completed: "Run finished",
  run_failed: "Run failed",
  run_cancelled: "Run cancelled",
  heartbeat: "Activity",
};

function Meta({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-[12px] text-muted">{label}</dt>
      <dd className="mt-0.5 text-[13.5px] text-fg-2">{children}</dd>
    </div>
  );
}

function Activity({ s }: { s: RunSignals }) {
  const items: Array<[string, string]> = [];
  if (s.turns > 0) {
    items.push(["Model responses", `${s.turns}${s.subagentTurns > 0 ? ` + ${s.subagentTurns} by subagents` : ""}`]);
  }
  if (s.outputTokens > 0) {
    const thinking = s.thinkingTokens > 0 ? ` (${formatTokens(s.thinkingTokens)} thinking)` : "";
    items.push(["Output tokens", `${formatTokens(s.outputTokens)}${thinking}`]);
  }
  if (s.contextTokens > 0) {
    items.push([
      "Context in use",
      s.contextWindow ? `${formatTokens(s.contextTokens)} of ${formatTokens(s.contextWindow)}` : formatTokens(s.contextTokens),
    ]);
  }
  if (s.compactions > 0) items.push(["Compactions", String(s.compactions)]);
  if (s.editLines > 0) items.push(["Lines edited", String(s.editLines)]);
  if (s.diffFiles != null) {
    items.push(["Working tree", `${s.diffFiles} ${s.diffFiles === 1 ? "file" : "files"}, ${s.diffLines ?? 0} lines changed`]);
  }
  if (items.length === 0) return null;
  return (
    <Section title="Run activity" description="Counted on this computer from the agent's transcript and from git. No text is kept.">
      <dl className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-4">
        {items.map(([label, value]) => (
          <Meta key={label} label={label}>
            {value}
          </Meta>
        ))}
      </dl>
    </Section>
  );
}

function EventRow({ e, start }: { e: EventView; start: number }) {
  const offset = Date.parse(e.sourceTimestamp) - start;
  const failed = e.eventType === "tool_failed" || e.eventType === "run_failed";
  return (
    <li className="grid grid-cols-[64px_1fr_auto] items-baseline gap-3 py-1.5 text-[13px]">
      <span className="tabular text-faint">+{formatClock(Math.max(0, offset))}</span>
      <span className={failed ? "text-danger" : "text-fg-2"}>
        {EVENT_LABELS[e.eventType]}
        {e.tool && <span className="ml-1.5 font-mono text-[12px] text-muted">{e.tool}</span>}
        {e.activity && e.activity !== "unknown" && <span className="ml-1.5 text-muted">· {ACTIVITY_LABELS[e.activity]}</span>}
      </span>
      <span className="tabular text-[12px] text-faint">{e.durationMs != null ? formatDuration(e.durationMs) : ""}</span>
    </li>
  );
}

export function RunDetail({ id }: { id: string }) {
  const goBack = useApp((s) => s.goBack);
  const live = useApp((s) => s.active[id]);
  const finishedTick = useApp((s) => s.finishedTick);
  const now = useNow();
  const [showAll, setShowAll] = useState(false);
  const detail = useAsync<Detail | null>(() => native().then((a) => a.runDetail(id)), [id, finishedTick, live?.forecast?.generatedAt]);

  const back = (
    <Button variant="ghost" size="sm" onClick={goBack} className="-ml-2 mb-4">
      <ChevronLeft size={14} /> Back
    </Button>
  );
  if (detail.loading && !detail.data) return <>{back}<Spinner /></>;
  if (detail.error) return <>{back}<Notice tone="warn">Could not load this run: {detail.error}</Notice></>;
  if (!detail.data) return <>{back}<Notice>This run no longer exists. It may have been deleted.</Notice></>;

  const d = detail.data;
  const run = live ?? d.run;
  const start = Date.parse(run.startedAt);
  const ended = run.state === "completed" || run.state === "failed" || run.state === "cancelled";
  const events = showAll ? d.events : d.events.filter((e) => e.eventType !== "heartbeat" && e.eventType !== "tool_started");
  const first = d.forecasts.find((f) => f.status === "available");
  const latest = run.forecast ?? [...d.forecasts].reverse().find((f) => f.status === "available");
  const actual = ended && (run.state === "completed" || run.state === "failed") ? run.activeMs / 1000 : null;

  return (
    <>
      {back}
      <header className="pb-6">
        <p className="text-[13px] text-muted">
          {AGENT_NAMES[run.agent]}
          {run.repositoryName && ` · ${run.repositoryName}`}
        </p>
        <h1 className="mt-1 text-[22px] font-semibold tracking-[-0.02em]">{runTitle(run)}</h1>
        <div className="mt-2">
          <StateLabel state={run.state} />
        </div>
      </header>

      {!ended && (
        <div className="mb-8">
          <RunCard run={run} now={now} note={d.changes.at(-1)?.message} showHeader={false} />
        </div>
      )}

      <dl className="mb-8 grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-4">
        <Meta label="Started">{new Date(start).toLocaleString()}</Meta>
        <Meta label={ended ? "Duration" : "Elapsed"}>{formatDuration(ended ? run.wallMs : now - start)}</Meta>
        <Meta label="Active / waiting">
          {formatDuration(run.activeMs)} / {formatDuration(run.waitingMs)}
        </Meta>
        <Meta label="Task type">{CATEGORY_LABELS[run.taskCategory]}</Meta>
        <Meta label="Tool calls">
          {run.toolCalls}
          {run.toolFailures > 0 && <span className="text-muted"> ({run.toolFailures} failed)</span>}
        </Meta>
        <Meta label="Initial estimate">
          {first?.remainingP10Seconds != null && first.remainingP50Seconds != null && first.remainingP90Seconds != null
            ? humanizeEta(first.remainingP10Seconds, first.remainingP50Seconds, first.remainingP90Seconds).likely
            : "None"}
        </Meta>
        {run.model && <Meta label="Model">{run.model}</Meta>}
        {run.effort && <Meta label="Effort">{effortLabel(run.effort)}</Meta>}
        {!ended && run.phase && <Meta label="Phase">{PHASE_LABELS[run.phase]}</Meta>}
        {ended && run.exitReason && <Meta label="How it ended">{EXIT_REASON_LABELS[run.exitReason] ?? run.exitReason}</Meta>}
      </dl>

      {run.state === "completed" && (
        <p className="mb-8 text-[12.5px] text-muted">
          “Run finished” means the agent reported the end of its turn. Whether the task itself succeeded is for you to judge.
        </p>
      )}

      {run.signals && <Activity s={run.signals} />}

      <Section title="Forecast history" description="How the predicted total duration changed as the run progressed.">
        <ForecastChart forecasts={d.forecasts} actualSeconds={actual} />
        {first && (
          <p className="mt-3 text-[12.5px] text-muted">
            {first.reasonCodes.map((r) => REASON_LABELS[r]).filter(Boolean).join(" · ")}
          </p>
        )}
        {latest && latest.components.length > 0 && (
          <p className="mt-1 text-[12.5px] text-muted">
            Models used: {latest.components.map((c) => COMPONENT_LABELS[c] ?? c).join(", ")}
            {latest.calibrated && " · range calibrated on your recent runs"}
          </p>
        )}
      </Section>

      {d.changes.length > 0 && (
        <Section title="Estimate changes">
          <ul className="space-y-1.5">
            {d.changes.map((c, i) => (
              <li key={i} className="grid grid-cols-[64px_1fr] gap-3 text-[13px]">
                <span className="tabular text-faint">+{formatClock(Date.parse(c.at) - start)}</span>
                <span className="text-fg-2">{c.message}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      <Section title="Phases">
        {d.phases.length === 0 ? (
          <p className="text-[13px] text-muted">No tool activity was reported for this run.</p>
        ) : (
          <ol className="relative ml-1 border-l border-line">
            {d.phases.map((p, i) => {
              const s = Date.parse(p.startedAt);
              const e = p.endedAt ? Date.parse(p.endedAt) : ended ? null : now;
              return (
                <li key={i} className="relative pb-3 pl-5 last:pb-0">
                  <span className="absolute -left-[4.5px] top-[7px] size-2 rounded-full border border-line-strong bg-surface" />
                  <div className="flex items-baseline justify-between gap-4 text-[13px]">
                    <span className="text-fg-2">{ACTIVITY_LABELS[p.activity]}</span>
                    <span className="tabular text-[12px] text-muted">
                      {e != null ? formatDuration(e - s) : ""}
                      {p.toolCalls > 0 && ` · ${p.toolCalls} ${p.toolCalls === 1 ? "call" : "calls"}`}
                    </span>
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </Section>

      <Section
        title="Events"
        description="Timing metadata only. Prompts, commands and file contents are never stored."
        actions={
          <Button variant="ghost" size="sm" onClick={() => setShowAll((v) => !v)}>
            {showAll ? "Show fewer" : `Show all ${d.events.length}`}
          </Button>
        }
      >
        <ul className="divide-y divide-line/60">
          {events.map((e) => (
            <EventRow key={e.id} e={e} start={start} />
          ))}
        </ul>
      </Section>
    </>
  );
}
