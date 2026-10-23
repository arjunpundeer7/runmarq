import type { Confidence, RunState } from "@runmarq/contracts";
import { CONFIDENCE_LABELS, STATE_LABELS } from "@runmarq/contracts";
import { cn } from "./cn";

type Tone = "active" | "waiting" | "ok" | "danger" | "neutral" | "unknown";

export function stateTone(state: RunState): Tone {
  switch (state) {
    case "created":
    case "estimating":
    case "running":
      return "active";
    case "waiting_for_user":
    case "waiting_for_external_resource":
    case "blocked":
      return "waiting";
    case "completed":
      return "ok";
    case "failed":
      return "danger";
    case "unknown":
      return "unknown";
    default:
      return "neutral";
  }
}

const DOT: Record<Tone, string> = {
  active: "bg-accent",
  waiting: "bg-warn",
  ok: "bg-ok",
  danger: "bg-danger",
  neutral: "bg-faint",
  unknown: "bg-transparent ring-1 ring-inset ring-faint",
};

/** A small state dot. Running states pulse gently (disabled under reduced motion). */
export function StatusDot({ state, className }: { state: RunState; className?: string }) {
  const tone = stateTone(state);
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block size-2 shrink-0 rounded-full", DOT[tone], tone === "active" && "rm-pulse", className)}
    />
  );
}

const BADGE: Record<Tone, string> = {
  active: "text-fg-2",
  waiting: "bg-warn-muted text-warn",
  ok: "text-ok",
  danger: "bg-danger-muted text-danger",
  neutral: "text-muted",
  unknown: "text-muted",
};

export function StateLabel({ state, className }: { state: RunState; className?: string }) {
  const tone = stateTone(state);
  const pill = tone === "waiting" || tone === "danger";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-[13px] font-medium",
        pill && "rounded-full px-2 py-0.5",
        BADGE[tone],
        className,
      )}
    >
      {!pill && <StatusDot state={state} />}
      {STATE_LABELS[state]}
    </span>
  );
}

/** Three bars, filled according to confidence. Always paired with a text label. */
export function ConfidenceMeter({ value, className }: { value: Confidence; className?: string }) {
  const filled = value === "high" ? 3 : value === "medium" ? 2 : 1;
  return (
    <span className={cn("inline-flex items-center gap-2", className)}>
      <span className="inline-flex items-end gap-[3px]" aria-hidden="true">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className={cn("w-[3px] rounded-[1px]", i < filled ? "bg-fg-2" : "bg-line-strong")}
            style={{ height: 6 + i * 3 }}
          />
        ))}
      </span>
      <span>{CONFIDENCE_LABELS[value]}</span>
    </span>
  );
}
