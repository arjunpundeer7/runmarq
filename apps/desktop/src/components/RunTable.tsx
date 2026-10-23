import type { RunView } from "@runmarq/contracts";
import { AGENT_NAMES, formatDuration, humanizeEta, STATE_LABELS } from "@runmarq/contracts";
import { cn, runTitle, StatusDot } from "@runmarq/ui";
import { useApp } from "../stores/app";

/** Whether the actual active duration fell inside the initial P10–P90 range. */
export function estimateOutcome(r: RunView): "inside" | "outside" | null {
  if (r.state !== "completed" && r.state !== "failed") return null;
  if (r.initialP10Seconds == null || r.initialP90Seconds == null) return null;
  const actual = r.activeMs / 1000;
  return actual >= r.initialP10Seconds && actual <= r.initialP90Seconds ? "inside" : "outside";
}

function when(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  return sameDay
    ? d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
        " " +
        d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

export function RunTable({ runs, dense }: { runs: RunView[]; dense?: boolean }) {
  const navigate = useApp((s) => s.navigate);
  return (
    <div className="overflow-hidden rounded-[var(--radius-card)] border border-line bg-surface">
      <table className="w-full table-fixed border-collapse text-[13px]">
        <thead>
          <tr className="border-b border-line text-left text-[12px] text-muted">
            <th className="w-[92px] px-4 py-2.5 font-normal">Started</th>
            <th className="px-3 py-2.5 font-normal">Run</th>
            {!dense && <th className="w-[130px] px-3 py-2.5 font-normal">Status</th>}
            <th className="w-[84px] px-3 py-2.5 text-right font-normal">Duration</th>
            <th className="w-[150px] px-4 py-2.5 text-right font-normal">Initial estimate</th>
          </tr>
        </thead>
        <tbody>
          {runs.map((r) => {
            const outcome = estimateOutcome(r);
            const est =
              r.initialP10Seconds != null && r.initialP50Seconds != null && r.initialP90Seconds != null
                ? humanizeEta(r.initialP10Seconds, r.initialP50Seconds, r.initialP90Seconds).likely.replace(/^About /, "")
                : "—";
            return (
              <tr
                key={r.id}
                onClick={() => navigate({ page: "run", id: r.id })}
                onKeyDown={(e) => (e.key === "Enter" ? navigate({ page: "run", id: r.id }) : undefined)}
                tabIndex={0}
                className="cursor-default border-b border-line last:border-b-0 hover:bg-surface-2/60 focus-visible:bg-surface-2"
              >
                <td className="px-4 py-2.5 tabular text-muted">{when(r.startedAt)}</td>
                <td className="truncate px-3 py-2.5">
                  <span className="inline-flex items-center gap-2">
                    {dense && <StatusDot state={r.state} />}
                    <span className="text-muted">{AGENT_NAMES[r.agent]}</span>
                    <span className="truncate text-fg">{runTitle(r)}</span>
                    {r.repositoryName && <span className="truncate text-faint">{r.repositoryName}</span>}
                  </span>
                </td>
                {!dense && (
                  <td className="px-3 py-2.5">
                    <span className="inline-flex items-center gap-2 text-fg-2">
                      <StatusDot state={r.state} />
                      {STATE_LABELS[r.state]}
                    </span>
                  </td>
                )}
                <td className="px-3 py-2.5 text-right tabular text-fg-2">{formatDuration(r.wallMs)}</td>
                <td className="px-4 py-2.5 text-right tabular">
                  <span className={cn(outcome === "outside" ? "text-warn" : "text-muted")}>{est}</span>
                  {outcome && (
                    <span className="sr-only">{outcome === "inside" ? ", actual inside the range" : ", actual outside the range"}</span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
