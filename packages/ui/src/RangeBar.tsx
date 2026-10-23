import { formatDuration, likelyRange } from "@runmarq/contracts";
import { cn } from "./cn";

export interface RangeBarProps {
  /** Active time already spent, in milliseconds. */
  elapsedMs: number;
  /** Remaining-time quantiles, in seconds. */
  p10: number;
  p50: number;
  p90: number;
  className?: string;
}

/**
 * Elapsed time as a solid segment, followed by the P10–P90 remaining band
 * with the likely P25–P75 part drawn stronger, and a tick at the median. The scale is the run's plausible total length,
 * so the bar answers "how far along are we, roughly" without a fake percentage.
 */
export function RangeBar({ elapsedMs, p10, p50, p90, className }: RangeBarProps) {
  const elapsed = Math.max(0, elapsedMs / 1000);
  const total = Math.max(1, elapsed + p90 * 1.08);
  const pct = (v: number) => `${Math.min(100, Math.max(0, (v / total) * 100))}%`;
  const [lo, hi] = likelyRange(p10, p50, p90);
  const label = `Elapsed ${formatDuration(elapsedMs)}. Likely finishes between ${formatDuration(lo * 1000)} and ${formatDuration(
    hi * 1000,
  )} from now, most likely around ${formatDuration(p50 * 1000)}, and could take up to ${formatDuration(p90 * 1000)}.`;
  return (
    <div className={cn("relative h-1.5 w-full rounded-full bg-surface-2", className)} role="img" aria-label={label}>
      <div className="absolute inset-y-0 left-0 rounded-full bg-fg-2/80" style={{ width: pct(elapsed) }} />
      <div
        className="absolute inset-y-0 rounded-full bg-accent/15 transition-[left,width] duration-700 ease-out"
        style={{ left: pct(elapsed + p10), width: `calc(${pct(elapsed + p90)} - ${pct(elapsed + p10)})` }}
      />
      <div
        className="absolute inset-y-0 rounded-full bg-accent/45 transition-[left,width] duration-700 ease-out"
        style={{ left: pct(elapsed + lo), width: `calc(${pct(elapsed + hi)} - ${pct(elapsed + lo)})` }}
      />
      <div
        className="absolute -inset-y-[3px] w-[2px] rounded-full bg-accent transition-[left] duration-700 ease-out"
        style={{ left: pct(elapsed + p50) }}
      />
    </div>
  );
}
