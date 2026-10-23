"use client";

import type { EtaForecast, RunView } from "@runmarq/contracts";
import { ACTIVITY_LABELS, AGENT_NAMES, CONFIDENCE_LABELS, etaText, formatClock, formatDuration, humanizeEta, likelyRange, REASON_LABELS } from "@runmarq/contracts";
import { cn } from "@runmarq/ui";
import { Activity, Check, ChartLine, Ellipsis, Info, ListTree, Pause, Play, RotateCcw, SquareTerminal } from "lucide-react";
import type { KeyboardEvent, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import demo from "@/content/demo-run.json";
import { AgentMark } from "./site";

interface Frame {
  t: number;
  label: string;
  detail: string;
  run: RunView;
  change: string | null;
}

const FRAMES = demo.frames as unknown as Frame[];
const LAST = FRAMES.length - 1;
const STEP_MS = 2200;

const TABS = [
  { id: "live", label: "Live run", Icon: Activity },
  { id: "estimates", label: "Estimates", Icon: ChartLine },
  { id: "timeline", label: "Timeline", Icon: ListTree },
  { id: "events", label: "Events", Icon: SquareTerminal },
] as const;
type Tab = (typeof TABS)[number]["id"];

function clock(t: number) {
  return `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
}

function usable(f: EtaForecast | null): f is EtaForecast & { remainingP10Seconds: number; remainingP50Seconds: number; remainingP90Seconds: number } {
  return f != null && f.status === "available" && f.remainingP10Seconds != null && f.remainingP50Seconds != null && f.remainingP90Seconds != null;
}

/** What the agent spent each stretch of the run on, up to frame `i`. */
function stretches(i: number, nowT: number) {
  const out: Array<{ label: string; s: number; waiting: boolean }> = [];
  for (let k = 0; k <= i; k++) {
    const end = k < i ? FRAMES[k + 1]!.t : nowT;
    const run = FRAMES[k]!.run;
    if (run.state === "completed") break;
    const waiting = run.state === "waiting_for_user";
    const label = waiting ? "Waiting for you" : run.currentActivity ? ACTIVITY_LABELS[run.currentActivity] : "Starting";
    const s = Math.max(0, end - FRAMES[k]!.t);
    const prev = out[out.length - 1];
    if (prev && prev.label === label) prev.s += s;
    else out.push({ label, s, waiting });
  }
  return out.filter((x) => x.s > 0);
}

/**
 * A sample run, replayed in a window shaped like the app. Every number was
 * produced by runmarq's own engine and estimator (crates/collector/examples/
 * demo_run.rs); the visitor can play it, scrub to any moment, or open the
 * run's estimates, timeline and events.
 */
export function DemoRun({ autoplay = false, className }: { autoplay?: boolean; className?: string }) {
  const [i, setI] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [touched, setTouched] = useState(false);
  const [tab, setTab] = useState<Tab>("live");
  const [progress, setProgress] = useState({ i: 0, f: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const frame = FRAMES[i]!;
  const run = frame.run;
  const done = i === LAST;
  const isPlaying = playing && !done;

  // Start once the window is on screen, unless the visitor prefers less motion or has already taken control.
  useEffect(() => {
    const el = rootRef.current;
    if (!autoplay || touched || !el || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const io = new IntersectionObserver(
      ([e]) => {
        if (e?.isIntersecting) {
          timer = setTimeout(() => setPlaying(true), 1600);
          io.disconnect();
        }
      },
      { threshold: 0.35 },
    );
    io.observe(el);
    return () => {
      io.disconnect();
      clearTimeout(timer);
    };
  }, [autoplay, touched]);

  // Advance one moment per step, and run the clock smoothly in between.
  useEffect(() => {
    if (!isPlaying) return;
    const start = performance.now();
    const tick = setInterval(() => setProgress({ i, f: Math.min(1, (performance.now() - start) / STEP_MS) }), 200);
    const next = setTimeout(() => setI((n) => Math.min(n + 1, LAST)), STEP_MS);
    return () => {
      clearInterval(tick);
      clearTimeout(next);
    };
  }, [isPlaying, i]);

  // Left to itself, the replay starts over after a pause on the finished run.
  useEffect(() => {
    if (!autoplay || touched || !playing || !done) return;
    const again = setTimeout(() => setI(0), 6000);
    return () => clearTimeout(again);
  }, [autoplay, touched, playing, done]);

  const f = isPlaying && progress.i === i ? progress.f : 0;
  const nowT = done ? run.wallMs / 1000 : frame.t + (FRAMES[i + 1]!.t - frame.t) * f;
  const eta = etaText(run);
  const lastForecast = [...FRAMES.slice(0, i + 1)].reverse().find((x) => usable(x.run.forecast))?.run.forecast ?? null;
  const lastChange = [...FRAMES.slice(0, i + 1)].reverse().find((x) => x.change)?.change ?? null;
  const note = frame.change ?? (run.state === "running" ? lastChange : null);

  const take = (fn: () => void) => {
    setTouched(true);
    fn();
  };
  const toggle = () =>
    take(() => {
      if (done) {
        setI(0);
        setPlaying(true);
      } else setPlaying(!isPlaying);
    });
  const onTabKey = (e: KeyboardEvent, n: number) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    const m = (n + (e.key === "ArrowRight" ? 1 : -1) + TABS.length) % TABS.length;
    setTab(TABS[m]!.id);
    tabRefs.current[m]?.focus();
  };

  return (
    <div ref={rootRef} data-ui className={cn("rm-window @container text-left text-fg", className)}>
      {/* Window chrome, titled so the replay is never mistaken for a live run. */}
      <div className="grid h-9 grid-cols-[1fr_auto_1fr] items-center px-4">
        <span className="flex gap-2" aria-hidden="true">
          <span className="rm-window-dot bg-[#ee6a5f]" />
          <span className="rm-window-dot bg-[#f5bd4f]" />
          <span className="rm-window-dot bg-[#61c454]" />
        </span>
        <span className="text-[12.5px] font-medium text-muted">Demo</span>
        <Ellipsis size={18} className="justify-self-end text-muted" aria-hidden="true" />
      </div>

      <div className="px-4 pb-4 sm:px-5">
        <header className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <span className="inline-flex size-10 shrink-0 items-center justify-center rounded-[10px] bg-[#f4dccd] dark:bg-[#4a2d1f]">
              <AgentMark agent="claude" size={22} />
            </span>
            <div className="min-w-0">
              <p className="truncate text-[12.5px] text-muted">
                {AGENT_NAMES[run.agent]} · {run.repositoryName}
              </p>
              <p className="line-clamp-2 text-[16px] font-medium leading-snug tracking-[-0.01em] @md:text-[17px]">{run.title}</p>
            </div>
          </div>
          <StatePill run={run} />
        </header>

        {/* Tabs */}
        <div role="tablist" aria-label="Run views" className="mt-3 grid grid-cols-4 gap-1 rounded-[12px] bg-surface-2 p-1">
          {TABS.map(({ id, label, Icon }, n) => (
            <button
              key={id}
              ref={(el) => {
                tabRefs.current[n] = el;
              }}
              role="tab"
              type="button"
              id={`run-tab-${id}`}
              aria-selected={tab === id}
              aria-controls={`run-panel-${id}`}
              tabIndex={tab === id ? 0 : -1}
              onClick={() => setTab(id)}
              onKeyDown={(e) => onTabKey(e, n)}
              className={cn(
                "flex h-8 min-w-0 items-center justify-center gap-1.5 rounded-[9px] px-1 text-[13px] transition-[background-color,color,box-shadow] duration-150",
                tab === id ? "bg-surface text-fg shadow-[0_1px_2px_rgb(60_35_20/0.1),0_0_0_1px_var(--rm-border)]" : "text-muted hover:text-fg",
              )}
            >
              <Icon size={15} className={cn("shrink-0", tab === id && "text-accent")} aria-hidden="true" />
              <span className="hidden truncate @sm:inline">{label}</span>
              <span className="sr-only @sm:hidden">{label}</span>
            </button>
          ))}
        </div>

        <div role="tabpanel" id={`run-panel-${tab}`} aria-labelledby={`run-tab-${tab}`} className="mt-3.5 min-h-[204px]">
          {tab === "live" && (
            <>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-4 @lg:grid-cols-3">
                <Stat label="Elapsed time">
                  <span className="tabular">{formatClock(nowT * 1000)}</span>
                </Stat>
                <Stat label={eta.kind === "estimate" ? "Estimate" : "Status"} divider>
                  <span className="block">{eta.headline}</span>
                  {eta.kind === "estimate" && <span className="block text-[12.5px] font-normal text-muted">{eta.detail}</span>}
                </Stat>
                <Stat label="Prediction confidence" divider className="col-span-2 @lg:col-span-1">
                  {usable(run.forecast) && eta.kind === "estimate" ? (
                    <span className="inline-flex items-center gap-2">
                      <Bars value={run.forecast.confidence} />
                      {CONFIDENCE_LABELS[run.forecast.confidence]}
                      <span
                        className="text-faint"
                        title={`${REASON_LABELS[run.forecast.reasonCodes[0] ?? ""] ?? "Based on local history"} · ${Math.round(run.forecast.sampleSize)} similar runs`}
                      >
                        <Info size={15} aria-hidden="true" />
                      </span>
                    </span>
                  ) : (
                    <span className="text-muted">—</span>
                  )}
                </Stat>
              </dl>

              <div className="mt-5">
                <div className="flex items-start gap-3">
                  <ActivityRing run={run} spinning={isPlaying} />
                  <div className="min-w-0">
                    <p className="text-[14.5px] leading-snug text-fg">{frame.detail || `${frame.label}.`}</p>
                    <p className={cn("mt-0.5 text-[12.5px]", run.state === "waiting_for_user" ? "text-warn" : run.state === "completed" ? "text-ok" : "text-accent-text")}>
                      {run.state === "waiting_for_user"
                        ? "Waiting for your approval"
                        : run.state === "completed"
                          ? `Actual duration ${formatDuration(run.wallMs)}`
                          : `${run.currentActivity ? ACTIVITY_LABELS[run.currentActivity] : "Starting"} · ${run.toolCalls} tool ${run.toolCalls === 1 ? "call" : "calls"}`}
                    </p>
                  </div>
                </div>
              </div>

              <ProgressTrack nowT={nowT} forecast={usable(run.forecast) ? run.forecast : lastForecast} done={done} waiting={run.state === "waiting_for_user"} />
              {note && <p className="mt-3 text-[12.5px] text-muted">{note}</p>}
            </>
          )}

          {tab === "estimates" && <EstimatesPanel i={i} />}
          {tab === "timeline" && <TimelinePanel i={i} nowT={nowT} />}
          {tab === "events" && <EventsPanel i={i} />}
        </div>

        {/* Replay controls and the moment they are on. */}
        <div className="mt-3 flex flex-col gap-2 rounded-[12px] bg-surface-2 px-3 py-2.5 @xl:flex-row @xl:items-center @xl:gap-4">
          <p className="min-w-0 flex-1 truncate text-[12.5px] leading-snug text-fg-2" aria-live="polite">
            <span className="tabular text-muted">{clock(frame.t)}</span> {frame.label}
          </p>
          <div className="flex items-center gap-3 @xl:w-[44%] @xl:shrink-0">
            <button
              type="button"
              onClick={toggle}
              aria-label={isPlaying ? "Pause" : done ? "Replay" : i === 0 ? "Play the run" : "Continue"}
              className="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-fg text-bg transition-transform duration-150 hover:scale-105 active:scale-95"
            >
              {isPlaying ? <Pause size={13} /> : done ? <RotateCcw size={13} /> : <Play size={13} className="translate-x-px" />}
            </button>
            <div role="group" aria-label="Moments in this run" className="flex h-8 min-w-0 flex-1 items-center gap-[3px]">
              {FRAMES.map((fr, n) => (
                <button
                  key={n}
                  type="button"
                  aria-label={`${clock(fr.t)} ${fr.label}`}
                  aria-current={n === i ? "step" : undefined}
                  title={`${clock(fr.t)} · ${fr.label}`}
                  onClick={() =>
                    take(() => {
                      setPlaying(false);
                      setI(n);
                    })
                  }
                  className="group flex h-full min-w-0 flex-1 items-center"
                >
                  <span
                    className={cn(
                      "h-1 w-full rounded-full transition-[background-color,height] duration-200 group-hover:h-2",
                      n === i ? "h-2 bg-accent" : n < i ? "bg-fg-2/70" : "bg-line-strong",
                    )}
                  />
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StatePill({ run }: { run: RunView }) {
  const waiting = run.state === "waiting_for_user";
  const finished = run.state === "completed";
  return (
    <span
      className={cn(
        "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full px-2.5 text-[12.5px] font-medium",
        waiting ? "bg-warn-muted text-warn" : "bg-ok-muted text-ok",
      )}
    >
      {finished ? <Check size={13} /> : <span className={cn("size-1.5 rounded-full", waiting ? "bg-warn" : "bg-ok rm-pulse")} />}
      {waiting ? "Waiting for you" : finished ? "Finished" : "Running"}
    </span>
  );
}

function Stat({ label, children, divider, className }: { label: string; children: ReactNode; divider?: boolean; className?: string }) {
  return (
    <div className={cn("min-w-0", divider && "@lg:border-l @lg:border-line @lg:pl-4", className)}>
      <dt className="text-[12.5px] text-muted">{label}</dt>
      <dd className="mt-1 text-[19px] font-medium leading-tight tracking-[-0.015em] @md:text-[21px]">{children}</dd>
    </div>
  );
}

function Bars({ value }: { value: "low" | "medium" | "high" }) {
  const filled = value === "high" ? 3 : value === "medium" ? 2 : 1;
  return (
    <span className="inline-flex items-end gap-[3px]" aria-hidden="true">
      {[0, 1, 2].map((n) => (
        <span key={n} className={cn("w-[4px] rounded-[1px]", n < filled ? "bg-accent" : "bg-line-strong")} style={{ height: 8 + n * 4 }} />
      ))}
    </span>
  );
}

function ActivityRing({ run, spinning }: { run: RunView; spinning: boolean }) {
  if (run.state === "completed") {
    return (
      <span className="mt-0.5 inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-ok-muted text-ok">
        <Check size={15} />
      </span>
    );
  }
  const waiting = run.state === "waiting_for_user";
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" className="mt-0.5 shrink-0" aria-hidden="true">
      <circle cx="14" cy="14" r="11" fill="none" stroke="var(--rm-border-strong)" strokeWidth="2.5" />
      <g className={spinning && !waiting ? "rm-spin" : undefined}>
        <circle
          cx="14"
          cy="14"
          r="11"
          fill="none"
          stroke={waiting ? "var(--rm-warn)" : "var(--rm-accent)"}
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeDasharray="48 70"
          transform="rotate(-90 14 14)"
        />
      </g>
    </svg>
  );
}

/** Elapsed time solid, then the P10–P90 band with its likely middle, and the median tick, on the run's plausible length. */
function ProgressTrack({ nowT, forecast, done, waiting }: { nowT: number; forecast: EtaForecast | null; done: boolean; waiting: boolean }) {
  const band = usable(forecast) ? forecast : null;
  const total = band ? nowT + band.remainingP90Seconds * 1.08 : Math.max(nowT, 1);
  const pct = (v: number) => `${Math.min(100, Math.max(0, (v / total) * 100))}%`;
  const label = done
    ? "Run finished."
    : band
      ? `Elapsed ${formatDuration(nowT * 1000)}. Likely ${humanizeEta(band.remainingP10Seconds, band.remainingP50Seconds, band.remainingP90Seconds).likely.toLowerCase()} more.`
      : `Elapsed ${formatDuration(nowT * 1000)}.`;
  return (
    <div className="relative mt-4 h-2 w-full rounded-full bg-surface-2" role="img" aria-label={label}>
      {band && !done && (
        <div
          className={cn("absolute inset-y-0 rounded-full transition-[left,width] duration-700 ease-out", waiting ? "bg-warn/15" : "bg-accent/15")}
          style={{ left: pct(nowT + band.remainingP10Seconds), width: `calc(${pct(nowT + band.remainingP90Seconds)} - ${pct(nowT + band.remainingP10Seconds)})` }}
        />
      )}
      {band && !done && (() => {
        const [lo, hi] = likelyRange(band.remainingP10Seconds, band.remainingP50Seconds, band.remainingP90Seconds);
        return (
          <div
            className={cn("absolute inset-y-0 rounded-full transition-[left,width] duration-700 ease-out", waiting ? "bg-warn/35" : "bg-accent/40")}
            style={{ left: pct(nowT + lo), width: `calc(${pct(nowT + hi)} - ${pct(nowT + lo)})` }}
          />
        );
      })()}
      <div
        className={cn("absolute inset-y-0 left-0 rounded-full transition-[width] duration-300 ease-out", done ? "bg-ok" : "bg-accent")}
        style={{ width: done ? "100%" : pct(nowT) }}
      />
      {band && !done && (
        <div
          className="absolute -inset-y-[4px] w-[2px] rounded-full bg-fg transition-[left] duration-700 ease-out"
          style={{ left: pct(nowT + band.remainingP50Seconds) }}
        />
      )}
    </div>
  );
}

function EstimatesPanel({ i }: { i: number }) {
  const rows = FRAMES.slice(0, i + 1)
    .map((fr) => ({ fr, fc: fr.run.forecast }))
    .filter((r) => usable(r.fc) && (r.fr.t === 0 || r.fr.change || r.fr.label === "Tests passed" || r.fr.label === "Editing code"))
    .reverse()
    .slice(0, 6);
  return (
    <div>
      <ul className="space-y-1">
        {rows.map(({ fr, fc }, n) => (
          <li key={fr.t} className={cn("grid grid-cols-[3rem_minmax(0,1fr)] gap-x-3 rounded-[9px] px-2.5 py-2 @md:grid-cols-[3rem_8.5rem_minmax(0,1fr)]", n === 0 && "bg-surface-2")}>
            <span className="tabular text-[13px] text-muted">{clock(fr.t)}</span>
            <span className="text-[13.5px] font-medium">{humanizeEta(fc!.remainingP10Seconds!, fc!.remainingP50Seconds!, fc!.remainingP90Seconds!).likely}</span>
            <span className="col-start-2 text-[12.5px] text-muted @md:col-start-auto">
              {fr.change ?? (fr.t === 0 ? "First estimate, from similar runs in this repository" : fr.label)}
            </span>
          </li>
        ))}
        {rows.length === 0 && <li className="px-2.5 py-2 text-[13px] text-muted">Paused while waiting for you.</li>}
      </ul>
    </div>
  );
}

function TimelinePanel({ i, nowT }: { i: number; nowT: number }) {
  const parts = stretches(i, nowT);
  const total = parts.reduce((s, p) => s + p.s, 0) || 1;
  return (
    <div>
      <div className="mt-1 flex h-2 overflow-hidden rounded-full bg-surface-2" aria-hidden="true">
        {parts.map((p, n) => (
          <span
            key={n}
            style={{ width: `${(p.s / total) * 100}%` }}
            className={cn(
              "h-full border-r-2 border-surface last:border-r-0",
              p.waiting ? "bg-warn/50" : p.label === "Running tests" ? "bg-accent" : p.label === "Editing code" ? "bg-fg-2" : "bg-faint",
            )}
          />
        ))}
      </div>
      <ul className="mt-3 space-y-1.5 text-[13px]">
        {parts
          .slice(-6)
          .reverse()
          .map((p, n) => (
            <li key={n} className="flex items-baseline justify-between gap-3">
              <span className={p.waiting ? "text-warn" : "text-fg-2"}>{p.label}</span>
              <span className="tabular text-muted">{formatClock(p.s * 1000)}</span>
            </li>
          ))}
      </ul>
    </div>
  );
}

function EventsPanel({ i }: { i: number }) {
  const rows = FRAMES.slice(0, i + 1).reverse().slice(0, 7);
  return (
    <div>
      <ul className="space-y-0.5">
        {rows.map((fr, n) => (
          <li key={fr.t} className={cn("grid grid-cols-[3rem_minmax(0,1fr)_auto] items-baseline gap-x-3 rounded-[9px] px-2.5 py-1.5 text-[13px]", n === 0 && "bg-surface-2")}>
            <span className="tabular text-muted">{clock(fr.t)}</span>
            <span className={cn("truncate", n === 0 ? "font-medium text-fg" : "text-fg-2")}>{fr.label}</span>
            <span className="text-[12px] text-muted">{fr.run.currentTool ?? ""}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
