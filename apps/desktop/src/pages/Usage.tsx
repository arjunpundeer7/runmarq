import type { AgentSource, AgentUsage, UsageTotals, UsageWindow } from "@runmarq/contracts";
import { AGENT_NAMES, formatDuration, formatTokens } from "@runmarq/contracts";
import { cn } from "@runmarq/ui";
import { useState } from "react";
import { Button, EmptyState, Notice, PageHeader, Section, Spinner } from "../components/controls";
import { useAsync } from "../hooks/useAsync";
import { native } from "../services/native";
import { useApp } from "../stores/app";

function sameDay(a: Date, b: Date): boolean {
  return a.toDateString() === b.toDateString();
}

/** "14:03" today, "Tue 14:03" on other days. */
function clock(iso: string): string {
  const d = new Date(iso);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return sameDay(d, new Date()) ? time : `${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

function Stat({ label, value, notes }: { label: string; value: string; notes: string[] }) {
  return (
    <div>
      <dt className="text-[12px] text-muted">{label}</dt>
      <dd className="mt-1 text-[22px] font-medium tracking-[-0.02em] tabular">{value}</dd>
      {notes.map((n) => (
        <dd key={n} className="mt-0.5 text-[12px] text-faint">
          {n}
        </dd>
      ))}
    </div>
  );
}

function totalsNotes(t: UsageTotals, tokens: boolean): string[] {
  const notes = [plural(t.runs, "run")];
  if (tokens && t.thinkingTokens > 0) notes[0] += ` · ${formatTokens(t.thinkingTokens)} thinking`;
  if (!tokens && t.activeMs > 0) notes[0] += ` · ${formatDuration(t.activeMs)} working`;
  return notes;
}

function value(t: UsageTotals, tokens: boolean): string {
  return tokens ? formatTokens(t.outputTokens) : plural(t.runs, "run");
}

function BudgetBar({ used, budget }: { used: number; budget: number }) {
  const pct = Math.round((used / budget) * 100);
  const over = used > budget;
  return (
    <div className="mt-5">
      <div
        role="meter"
        aria-label="Share of your window budget used"
        aria-valuemin={0}
        aria-valuemax={budget}
        aria-valuenow={used}
        className="h-1.5 overflow-hidden rounded-full bg-surface-2"
      >
        <div className={cn("h-full rounded-full", over ? "bg-warn" : "bg-accent")} style={{ width: `${Math.min(pct, 100)}%` }} />
      </div>
      <p className={cn("mt-1.5 text-[12.5px] tabular", over ? "text-warn" : "text-muted")}>
        {over
          ? `${formatTokens(used - budget)} over your ${formatTokens(budget)} budget for this window`
          : `${formatTokens(used)} of your ${formatTokens(budget)} budget (${pct}%)`}
      </p>
    </div>
  );
}

function BudgetEditor({ agent, budget }: { agent: AgentSource; budget: number | null }) {
  const prefs = useApp((s) => s.preferences);
  const save = useApp((s) => s.savePreferences);
  const [draft, setDraft] = useState("");
  if (!prefs) return null;
  const set = (v: number | null) => {
    const next = { ...prefs.usageWindowBudgets };
    if (v == null) delete next[agent];
    else next[agent] = v;
    void save({ usageWindowBudgets: next });
    setDraft("");
  };
  const parsed = Math.round(Number(draft.replace(/[\s,_]/g, "")));
  const valid = draft.trim() !== "" && Number.isFinite(parsed) && parsed > 0;
  const id = `budget-${agent}`;
  return (
    <div className="mt-5 flex flex-wrap items-center gap-2 text-[12.5px] text-muted">
      <label htmlFor={id}>{budget == null ? "Your budget per window" : "Change your budget"}</label>
      <input
        id={id}
        inputMode="numeric"
        placeholder="Output tokens"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && valid) set(parsed);
        }}
        className="h-7 w-32 rounded-lg border border-line-strong bg-surface px-2 text-[12.5px] text-fg outline-none focus-visible:border-accent"
      />
      <Button size="sm" disabled={!valid} onClick={() => set(parsed)}>
        Set
      </Button>
      {budget != null && (
        <Button size="sm" variant="ghost" onClick={() => set(null)}>
          Remove budget
        </Button>
      )}
    </div>
  );
}

function windowNotes(w: UsageWindow, tokens: boolean): string[] {
  return [...totalsNotes(w.totals, tokens), `Started ${clock(w.startedAt)}, resets about ${clock(w.endsAt)}`];
}

function AgentSection({ a }: { a: AgentUsage }) {
  const prefs = useApp((s) => s.preferences);
  const tokens = a.reportsTokens;
  const budget = prefs ? (prefs.usageWindowBudgets[a.agent] ?? null) : a.windowBudgetTokens;
  const w = a.currentWindow;
  return (
    <Section
      title={AGENT_NAMES[a.agent]}
      description={
        tokens ? undefined : `${AGENT_NAMES[a.agent]} does not write token counts runmarq can read, so only runs and working time are shown.`
      }
    >
      <dl className="grid grid-cols-1 gap-6 sm:grid-cols-3">
        {w ? (
          <Stat label="This 5-hour window" value={value(w.totals, tokens)} notes={windowNotes(w, tokens)} />
        ) : (
          <Stat label="This 5-hour window" value="None open" notes={["The next one opens with your next run"]} />
        )}
        <Stat label="Today" value={value(a.today, tokens)} notes={totalsNotes(a.today, tokens)} />
        <Stat label="Last 7 days" value={value(a.lastSevenDays, tokens)} notes={totalsNotes(a.lastSevenDays, tokens)} />
      </dl>
      {tokens && budget != null && w && <BudgetBar used={w.totals.outputTokens} budget={budget} />}
      {a.recentWindows.length > 0 && (
        <div className="mt-6">
          <h3 className="mb-2 text-[13px] font-medium">Earlier windows</h3>
          <ul className="divide-y divide-line/60 text-[13px]">
            {a.recentWindows.slice(0, 8).map((x) => (
              <li key={x.startedAt} className="flex items-center justify-between gap-4 py-1.5">
                <span className="text-fg-2 tabular">
                  {clock(x.startedAt)} – {clock(x.endsAt)}
                </span>
                <span className="text-muted tabular">
                  {tokens ? `${formatTokens(x.totals.outputTokens)} · ` : ""}
                  {plural(x.totals.runs, "run")}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {tokens && <BudgetEditor agent={a.agent} budget={budget} />}
    </Section>
  );
}

export function Usage() {
  const tick = useApp((s) => s.finishedTick);
  const navigate = useApp((s) => s.navigate);
  const usage = useAsync(() => native().then((a) => a.usage()), [tick]);
  const r = usage.data;
  return (
    <>
      <PageHeader
        title="Usage"
        description="Output tokens your agents recorded in their session transcripts on this computer. This is not your plan's usage meter: input and cached tokens are not included, and runmarq does not know your plan's limits."
        actions={
          <Button size="sm" variant="ghost" onClick={usage.reload}>
            Refresh
          </Button>
        }
      />
      {usage.error ? (
        <Notice tone="warn">Usage is unavailable: {usage.error}</Notice>
      ) : !r ? (
        <Spinner />
      ) : (
        <>
          {!r.transcriptsEnabled && (
            <div className="mb-6">
              <Notice tone="warn">
                Reading agent transcripts is turned off, so no new token counts are recorded.{" "}
                <button type="button" className="underline" onClick={() => navigate({ page: "settings" })}>
                  Change this in Settings
                </button>
              </Notice>
            </div>
          )}
          {r.agents.length === 0 ? (
            <EmptyState title="No agent runs in the last 7 days.">Usage appears here once a connected agent has run.</EmptyState>
          ) : (
            <>
              {r.agents.map((a) => (
                <AgentSection key={a.agent} a={a} />
              ))}
              <p className="border-t border-line pt-4 text-[12px] leading-relaxed text-faint">
                Claude plans count usage in {r.windowHours}-hour windows that open with your first message after the previous window
                closed. runmarq applies the same windows to every agent and infers them from run activity, so start and reset times are
                approximate. A run that crosses into a new window is split between them by time.
              </p>
            </>
          )}
        </>
      )}
    </>
  );
}
