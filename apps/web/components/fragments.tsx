// True-size fragments of the runmarq interface, built from the same
// components and wording as the desktop app. Each is marked `data-ui`.
import { formatDuration, humanizeEta, OUTLOOK_LABELS, outlookOf } from "@runmarq/contracts";
import { cn, LogoMark, RangeBar, StatusDot } from "@runmarq/ui";
import { Check } from "lucide-react";

export function RangeExplainer() {
  const elapsed = 260_000;
  const eta = humanizeEta(300, 510, 840);
  return (
    <div data-ui className="text-fg w-full max-w-[520px] rounded-[12px] bg-surface p-6 shadow-soft">
      <p className="text-[13px] text-muted">Estimate</p>
      <p className="mt-1 text-[26px] font-medium leading-tight tracking-[-0.015em]">{eta.likely}</p>
      <p className="mt-0.5 text-[14px] text-fg-2">{OUTLOOK_LABELS[outlookOf(300, 510, 840)]}</p>
      <RangeBar className="mt-6" elapsedMs={elapsed} p10={300} p50={510} p90={840} />
      <dl className="mt-5 grid grid-cols-2 gap-4 text-[13px] sm:grid-cols-3">
        <div>
          <dt className="text-muted">Elapsed</dt>
          <dd className="mt-0.5 text-fg-2">{formatDuration(elapsed)}</dd>
        </div>
        <div>
          <dt className="text-muted">Most likely</dt>
          <dd className="mt-0.5 text-fg-2">About {formatDuration(510_000)} more</dd>
        </div>
        <div>
          <dt className="text-muted">Prediction confidence</dt>
          <dd className="mt-0.5 text-fg-2">Medium</dd>
        </div>
      </dl>
    </div>
  );
}

export function TrayMenu({ className }: { className?: string }) {
  const rows = [
    { agent: "Claude Code", repo: "acme-web", text: "A few minutes, about 6–10 min", state: "running" as const },
    { agent: "Codex", repo: "billing-api", text: "Waiting for approval", state: "waiting_for_user" as const },
  ];
  return (
    <div data-ui className={cn("text-fg w-full max-w-[300px] rounded-[10px] bg-surface p-1.5 text-[13.5px] shadow-soft", className)} role="presentation">
      <p className="flex items-center gap-2 px-2.5 py-1.5 font-medium">
        <LogoMark size={15} /> runmarq
      </p>
      {rows.map((r) => (
        <div key={r.agent} className="flex items-center gap-2 rounded-md px-2.5 py-1.5">
          <StatusDot state={r.state} />
          <span className="min-w-0 truncate">
            <span className="text-fg">{r.agent}</span> <span className="text-muted">{r.repo}</span>
            <span className={cn("block", r.state === "waiting_for_user" ? "text-warn" : "text-muted")}>{r.text}</span>
          </span>
        </div>
      ))}
      <div className="my-1.5 h-px bg-line" />
      <p className="rounded-md bg-surface-2 px-2.5 py-1.5">Open runmarq</p>
      <p className="px-2.5 py-1.5 text-fg-2">Show compact panel</p>
      <p className="px-2.5 py-1.5 text-fg-2">Quiet mode</p>
    </div>
  );
}

export function NotificationToast({ className }: { className?: string }) {
  return (
    <div data-ui className={cn("text-fg flex w-full max-w-[340px] gap-3 rounded-[12px] bg-surface p-4 shadow-soft", className)}>
      <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-lg bg-surface-2 ring-1 ring-line">
        <LogoMark size={17} />
      </span>
      <div className="min-w-0 text-[13.5px] leading-snug">
        <p className="font-medium">Claude Code finished</p>
        <p className="mt-0.5 text-fg-2">Run in acme-web · 7 min</p>
      </div>
    </div>
  );
}

export function PhaseTimeline() {
  const phases = [
    { label: "Reading files", ms: 56_000, calls: 6 },
    { label: "Editing code", ms: 76_000, calls: 4 },
    { label: "Running tests", ms: 192_000, calls: 3, note: "2 failed" },
    { label: "Waiting for you", ms: 42_000, calls: 0, waiting: true },
    { label: "Editing code", ms: 38_000, calls: 2 },
    { label: "Running tests", ms: 34_000, calls: 1 },
  ];
  const total = phases.reduce((s, p) => s + p.ms, 0);
  return (
    <div data-ui className="text-fg w-full max-w-[560px] rounded-[12px] bg-surface p-6 shadow-soft">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <p className="text-[14px] font-medium">Phases</p>
        <p className="text-[13px] text-muted">Run finished · {formatDuration(total)}</p>
      </div>
      <div className="mt-4 flex h-2 overflow-hidden rounded-full" aria-hidden="true">
        {phases.map((p, i) => (
          <span
            key={i}
            style={{ width: `${(p.ms / total) * 100}%` }}
            className={cn(
              "h-full border-r-2 border-surface last:border-r-0",
              p.waiting ? "bg-warn/50" : p.label === "Running tests" ? "bg-accent" : p.label === "Editing code" ? "bg-fg-2" : "bg-faint",
            )}
          />
        ))}
      </div>
      <ul className="mt-5 space-y-2 text-[13.5px]">
        {phases.map((p, i) => (
          <li key={i} className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <span className={p.waiting ? "text-warn" : "text-fg-2"}>{p.label}</span>
            <span className="tabular text-muted sm:text-right">
              {formatDuration(p.ms)}
              {p.calls > 0 && ` · ${p.calls} ${p.calls === 1 ? "call" : "calls"}`}
              {p.note && <span className="text-danger"> · {p.note}</span>}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function ConnectRow({ connected }: { connected: boolean }) {
  return (
    <div data-ui className="text-fg w-full max-w-[280px] rounded-[12px] bg-surface p-4 shadow-soft">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <p className="text-[14px] font-medium">Claude Code</p>
        {connected ? (
          <span className="inline-flex items-center gap-1 text-[13px] font-medium text-ok">
            <Check size={13} /> Connected
          </span>
        ) : (
          <span className="text-[13px] text-muted">Detected · 2.1.143</span>
        )}
      </div>
      <span
        className={cn(
          "mt-4 inline-flex h-8 items-center rounded-lg px-3 text-[13px] font-medium",
          connected ? "border border-line-strong text-fg-2" : "bg-fg text-bg",
        )}
      >
        {connected ? "Re-run setup" : "Approve and connect"}
      </span>
    </div>
  );
}

/** The Insights "Estimation model" section, as it reads after 64 finished runs. */
export function ModelPanel({ className }: { className?: string }) {
  const models = [
    { label: "History baseline", runs: "64 runs", active: true },
    { label: "Phase simulation", runs: "61 runs", active: true },
    { label: "Similar prompts", runs: "64 runs", active: true },
    { label: "Learned model", runs: "64 of 100 runs", active: false },
  ];
  return (
    <div data-ui className={cn("text-fg w-full max-w-[540px] rounded-[12px] bg-surface p-6 shadow-soft", className)}>
      <p className="text-[14px] font-medium">Estimation model</p>
      <ul className="mt-3 divide-y divide-line/60">
        {models.map((m) => (
          <li key={m.label} className="grid grid-cols-[minmax(0,1fr)_auto_5.5rem] items-baseline gap-4 py-2.5 text-[13.5px]">
            <span className={cn("font-medium", !m.active && "text-muted")}>{m.label}</span>
            <span className="tabular text-[12.5px] text-muted">{m.runs}</span>
            <span className={cn("text-right text-[12px]", m.active ? "text-accent-text" : "text-muted")}>{m.active ? "In use" : "Not in use"}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The run detail "Run activity" section: numbers only, read on this computer. */
export function RunActivity({ className }: { className?: string }) {
  const items = [
    ["Effort", "High"],
    ["Phase", "Editing"],
    ["Model responses", "35 + 6 by subagents"],
    ["Output tokens", "16.4k (2.1k thinking)"],
    ["Context in use", "84k of 200k"],
    ["Compactions", "1"],
    ["Lines edited", "118"],
    ["Working tree", "4 files, 132 lines changed"],
  ];
  return (
    <div data-ui className={cn("text-fg w-full max-w-[560px] rounded-[12px] bg-surface p-6 shadow-soft", className)}>
      <p className="text-[14px] font-medium">Run activity</p>
      <dl className="mt-4 grid grid-cols-2 gap-x-8 gap-y-4">
        {items.map(([label, value]) => (
          <div key={label}>
            <dt className="text-[12px] text-muted">{label}</dt>
            <dd className="mt-0.5 text-[13.5px] text-fg-2">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/** Settings → Notifications, with its defaults. */
export function NotifyPanel({ className }: { className?: string }) {
  const rows = [
    { label: "Quiet mode", on: false },
    { label: "When a run finishes", on: true },
    { label: "When a run fails", on: true },
    { label: "When an agent needs approval", on: true },
    { label: "When an agent stops reporting", on: true },
    { label: "When a run looks stuck", on: true },
    { label: "Play a sound", on: true },
  ];
  return (
    <div data-ui className={cn("text-fg w-full max-w-[340px] rounded-[12px] bg-surface p-5 shadow-soft", className)}>
      <p className="text-[14px] font-medium">Notifications</p>
      <ul className="mt-2 divide-y divide-line/60 text-[13px]">
        {rows.map((r) => (
          <li key={r.label} className="flex items-center justify-between gap-4 py-2.5">
            <span className="text-fg-2">{r.label}</span>
            <span aria-hidden="true" className={cn("relative h-[18px] w-[30px] shrink-0 rounded-full", r.on ? "bg-accent" : "bg-line-strong")}>
              <span className={cn("absolute top-[2px] size-[14px] rounded-full bg-surface", r.on ? "left-[14px]" : "left-[2px]")} />
            </span>
          </li>
        ))}
        <li className="flex items-center justify-between gap-4 py-2.5">
          <span className="text-fg-2">Skip very short runs</span>
          <span className="text-muted">20 seconds</span>
        </li>
      </ul>
    </div>
  );
}

/** The Usage page: counts read from local transcripts, never the vendor's meter. */
export function UsagePanel({ className }: { className?: string }) {
  const stats = [
    { label: "5-hour window", value: "126k" },
    { label: "Today", value: "212k" },
    { label: "Last 7 days", value: "1.4M" },
  ];
  return (
    <div data-ui className={cn("text-fg w-full max-w-[560px] rounded-[12px] bg-surface p-6 shadow-soft", className)}>
      <p className="text-[14px] font-medium">Claude Code output tokens</p>
      <dl className="mt-4 grid grid-cols-3 gap-5">
        {stats.map((s) => (
          <div key={s.label}>
            <dt className="text-[12px] text-muted">{s.label}</dt>
            <dd className="mt-0.5 text-[19px] font-medium tracking-[-0.01em]">{s.value}</dd>
          </div>
        ))}
      </dl>
      <div className="mt-5 flex items-center gap-3 text-[12px] text-muted">
        <div
          role="img"
          aria-label="126k of a 200k budget"
          className="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-2"
        >
          <div className="h-full w-[63%] rounded-full bg-accent" />
        </div>
        <span className="tabular">126k / 200k</span>
      </div>
    </div>
  );
}

/** `runmarq wait` and `runmarq statusline` in a terminal. */
export function TerminalPanel({ className }: { className?: string }) {
  return (
    <div data-ui className={cn("w-full max-w-[560px] rounded-[12px] bg-code p-5 font-mono text-[13px] leading-relaxed text-code-fg shadow-soft ring-1 ring-code-edge", className)}>
      <p>
        <span className="text-code-fg/50">$</span> runmarq wait &amp;&amp; npm run deploy
      </p>
      <p className="text-code-fg/60">Waiting for Claude Code · acme-web · 4:12 elapsed · A few minutes, about 2–5 min</p>
      <p>✓ Claude Code · acme-web finished after 6:12.</p>
      <p className="mt-3">
        <span className="text-code-fg/50">$</span> runmarq statusline
      </p>
      <p>runmarq ● 2 working · next: about 3–7 min</p>
    </div>
  );
}

export function MiniRun({ text, detail, sub, state }: { text: string; detail?: string; sub: string; state: "running" | "completed" }) {
  return (
    <div data-ui className="text-fg w-full max-w-[280px] rounded-[12px] bg-surface p-4 shadow-soft">
      <p className="text-[13px] text-muted">
        <span className="font-medium text-fg-2">Claude Code</span> · acme-web
      </p>
      <p className="mt-1 text-[14px] font-medium">Feature</p>
      <p className="mt-1 flex items-center gap-1.5 text-[13px] text-muted">
        <StatusDot state={state} /> {sub}
      </p>
      <p className="mt-3 text-[12px] text-muted">{state === "running" ? "Estimate" : "Actual duration"}</p>
      <p className="text-[18px] font-medium tracking-[-0.01em]">{text}</p>
      {detail && <p className="text-[13px] text-fg-2">{detail}</p>}
    </div>
  );
}
