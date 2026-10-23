import type { EtaForecast } from "@runmarq/contracts";
import { formatDuration } from "@runmarq/contracts";

/**
 * Predicted total duration over the life of a run: for each forecast, the
 * P10–P90 band and the median of (elapsed + remaining). A horizontal line
 * marks the actual duration once the run has ended, which makes it easy to
 * see when the estimate converged.
 */
export function ForecastChart({ forecasts, actualSeconds }: { forecasts: EtaForecast[]; actualSeconds: number | null }) {
  const pts = forecasts
    .filter((f) => f.status === "available" && f.remainingP10Seconds != null && f.remainingP50Seconds != null && f.remainingP90Seconds != null)
    .map((f) => ({
      x: f.activeElapsedSeconds,
      lo: f.activeElapsedSeconds + f.remainingP10Seconds!,
      mid: f.activeElapsedSeconds + f.remainingP50Seconds!,
      hi: f.activeElapsedSeconds + f.remainingP90Seconds!,
    }));
  if (pts.length < 2) {
    return <p className="text-[13px] text-muted">The chart appears once the run has more than one estimate.</p>;
  }
  const W = 640;
  const H = 180;
  const pad = { l: 44, r: 12, t: 10, b: 24 };
  const maxX = Math.max(...pts.map((p) => p.x), actualSeconds ?? 0, 1);
  const maxY = Math.max(...pts.map((p) => p.hi), actualSeconds ?? 0, 1) * 1.05;
  const sx = (v: number) => pad.l + (v / maxX) * (W - pad.l - pad.r);
  const sy = (v: number) => H - pad.b - (v / maxY) * (H - pad.t - pad.b);
  const band = [...pts.map((p) => `${sx(p.x)},${sy(p.hi)}`), ...[...pts].reverse().map((p) => `${sx(p.x)},${sy(p.lo)}`)].join(" ");
  const mid = pts.map((p, i) => `${i === 0 ? "M" : "L"}${sx(p.x)},${sy(p.mid)}`).join(" ");
  const ticks = [0, 0.5, 1].map((t) => t * maxY);

  return (
    <figure>
      <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img" aria-label="Predicted total duration over the course of the run">
        {ticks.map((t) => (
          <g key={t}>
            <line x1={pad.l} x2={W - pad.r} y1={sy(t)} y2={sy(t)} stroke="var(--rm-border)" strokeWidth="1" />
            <text x={pad.l - 6} y={sy(t) + 4} textAnchor="end" fontSize="10.5" fill="var(--rm-faint)">
              {formatDuration(t * 1000)}
            </text>
          </g>
        ))}
        <polygon points={band} fill="var(--rm-accent)" opacity="0.14" />
        <path d={mid} fill="none" stroke="var(--rm-accent)" strokeWidth="1.75" strokeLinejoin="round" />
        {actualSeconds != null && (
          <>
            <line
              x1={pad.l}
              x2={W - pad.r}
              y1={sy(actualSeconds)}
              y2={sy(actualSeconds)}
              stroke="var(--rm-fg-2)"
              strokeWidth="1"
              strokeDasharray="3 3"
            />
            <text x={W - pad.r} y={sy(actualSeconds) - 5} textAnchor="end" fontSize="10.5" fill="var(--rm-fg-2)">
              Actual {formatDuration(actualSeconds * 1000)}
            </text>
          </>
        )}
        <text x={pad.l} y={H - 6} fontSize="10.5" fill="var(--rm-faint)">
          Start
        </text>
        <text x={W - pad.r} y={H - 6} textAnchor="end" fontSize="10.5" fill="var(--rm-faint)">
          {formatDuration(maxX * 1000)} active
        </text>
      </svg>
      <figcaption className="mt-2 flex gap-4 text-[12px] text-muted">
        <span className="inline-flex items-center gap-1.5">
          <span className="h-[2px] w-4 rounded bg-accent" /> Predicted total (median)
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="h-2.5 w-4 rounded-sm bg-accent/15" /> P10–P90 range
        </span>
      </figcaption>
    </figure>
  );
}
