import type { RunView } from "@runmarq/contracts";
import { ACTIVITY_LABELS, AGENT_NAMES, CATEGORY_LABELS, etaText, formatClock, liveElapsedMs } from "@runmarq/contracts";
import type { ReactNode } from "react";
import { cn } from "./cn";
import { RangeBar } from "./RangeBar";
import { ConfidenceMeter, StateLabel, StatusDot } from "./Status";

export type RunCardData = Pick<
  RunView,
  | "id"
  | "agent"
  | "repositoryName"
  | "title"
  | "taskCategory"
  | "promptKnown"
  | "state"
  | "startedAt"
  | "endedAt"
  | "wallMs"
  | "activeMs"
  | "currentActivity"
  | "attention"
  | "attentionDetail"
  | "forecast"
  | "plan"
>;

export interface RunCardProps {
  run: RunCardData;
  /** Current time in ms, so a parent can drive one clock for many cards. */
  now?: number;
  /** A short, factual line explaining the latest significant estimate change. */
  note?: string | null;
  /** Shown in the corner, for example "Demonstration" on the website. */
  tag?: ReactNode;
  /** Optional actions (open details, etc.). */
  footer?: ReactNode;
  compact?: boolean;
  /** Hide agent, repository and title when the surrounding page already shows them. */
  showHeader?: boolean;
  className?: string;
}

export function runTitle(run: Pick<RunView, "title" | "taskCategory" | "promptKnown">): string {
  if (run.title) return run.title;
  if (!run.promptKnown) return "Run started without a prompt event";
  return CATEGORY_LABELS[run.taskCategory] ?? "Run";
}

/**
 * The primary run display: what is running, how long it has taken, and
 * when it will likely finish, with its uncertainty stated plainly.
 */
export function RunCard({ run, now, note, tag, footer, compact, showHeader = true, className }: RunCardProps) {
  const elapsed = liveElapsedMs(run, now);
  const eta = etaText(run);
  const f = run.forecast;
  const activeMs = run.endedAt ? run.activeMs : Math.max(run.activeMs, elapsed - (run.wallMs - run.activeMs));
  const showRange =
    eta.kind === "estimate" && f?.remainingP10Seconds != null && f.remainingP50Seconds != null && f.remainingP90Seconds != null;
  const activity = run.currentActivity ? ACTIVITY_LABELS[run.currentActivity] : null;
  const planPct =
    run.plan && run.plan.totalSteps >= 2 ? Math.round((run.plan.completedSteps / run.plan.totalSteps) * 100) : null;

  return (
    <article
      className={cn(
        "@container rounded-[var(--radius-card)] border border-line bg-surface text-fg shadow-soft",
        compact ? "p-4" : "p-5 sm:p-6",
        className,
      )}
      aria-label={`${AGENT_NAMES[run.agent]} run`}
    >
      {showHeader && (
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="flex items-center gap-2 text-[13px] text-muted">
            <span className="shrink-0 whitespace-nowrap font-medium text-fg-2">{AGENT_NAMES[run.agent]}</span>
            {run.repositoryName && (
              <>
                <span aria-hidden="true">·</span>
                <span className="truncate">{run.repositoryName}</span>
              </>
            )}
          </p>
          <h3 className={cn("mt-1 line-clamp-2 font-medium tracking-[-0.01em]", compact ? "text-[15px]" : "text-[17px]")}>
            {runTitle(run)}
          </h3>
        </div>
        {tag && <div className="shrink-0">{tag}</div>}
      </header>
      )}

      <p className={cn(showHeader && "mt-2", "flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-muted")}>
        <StateLabel state={run.state} />
        <span aria-hidden="true">·</span>
        <span className="tabular">{formatClock(elapsed)} elapsed</span>
        {run.wallMs - run.activeMs > 5_000 && (
          <>
            <span aria-hidden="true">·</span>
            <span className="tabular">{formatClock(run.wallMs - run.activeMs)} waiting</span>
          </>
        )}
      </p>

      <dl className={cn("grid gap-x-6 gap-y-4", compact ? "mt-4 grid-cols-2" : "mt-5 grid-cols-2 @xl:grid-cols-3")}>
        <div className="col-span-2 @xl:col-span-1">
          <dt className="text-[12px] text-muted">{eta.kind === "estimate" ? "Estimate" : "Status"}</dt>
          <dd className={cn("mt-1 font-medium tracking-[-0.015em]", compact ? "text-[17px]" : "text-[22px] leading-tight")}>
            {eta.headline}
          </dd>
          {eta.detail && (
            <dd className="mt-0.5 text-[13px] text-fg-2">
              {eta.detail}
              {eta.kind === "estimate" && eta.tail && <span className="text-muted"> · {eta.tail}</span>}
            </dd>
          )}
        </div>
        <div>
          <dt className="text-[12px] text-muted">Current activity</dt>
          <dd className="mt-1 flex items-center gap-2 text-[14px] text-fg-2">
            {run.state === "waiting_for_user" ? "Waiting for approval" : activity ?? "—"}
          </dd>
        </div>
        <div>
          <dt className="text-[12px] text-muted">{planPct != null ? "Plan" : "Prediction confidence"}</dt>
          <dd className="mt-1 text-[14px] text-fg-2">
            {planPct != null ? (
              <span className="tabular">
                {run.plan!.completedSteps} of {run.plan!.totalSteps} steps
              </span>
            ) : f && eta.kind === "estimate" ? (
              <ConfidenceMeter value={f.confidence} />
            ) : (
              "—"
            )}
          </dd>
        </div>
      </dl>

      {showRange && (
        <RangeBar
          className="mt-5"
          elapsedMs={activeMs}
          p10={f!.remainingP10Seconds!}
          p50={f!.remainingP50Seconds!}
          p90={f!.remainingP90Seconds!}
        />
      )}

      {run.attention === "approval_required" && (
        <p className="mt-4 flex items-center gap-2 rounded-lg bg-warn-muted px-3 py-2 text-[13px] text-warn">
          <StatusDot state="waiting_for_user" />
          {AGENT_NAMES[run.agent]} is waiting for your approval.
        </p>
      )}
      {run.attention === "connection_lost" && (
        <p className="mt-4 rounded-lg bg-surface-2 px-3 py-2 text-[13px] text-muted">
          No updates from {AGENT_NAMES[run.agent]} recently. runmarq will resume when the agent sends another event.
        </p>
      )}
      {run.attention === "stuck" && (
        <p className="mt-4 rounded-lg bg-warn-muted px-3 py-2 text-[13px] text-warn">
          {AGENT_NAMES[run.agent]} may be stuck.
          {run.attentionDetail && ` ${run.attentionDetail}.`}
        </p>
      )}
      {note && <p className="mt-4 text-[13px] text-muted">{note}</p>}
      {footer && <div className="mt-4 border-t border-line pt-3">{footer}</div>}
    </article>
  );
}
