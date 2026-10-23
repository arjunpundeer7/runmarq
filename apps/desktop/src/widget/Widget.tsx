import { AGENT_NAMES, etaText, formatClock, liveElapsedMs } from "@runmarq/contracts";
import { cn, StatusDot } from "@runmarq/ui";
import { ChevronDown, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useNow } from "../hooks/useNow";
import { native } from "../services/native";
import { useShallow } from "zustand/react/shallow";
import { selectActiveRuns, useApp } from "../stores/app";

/**
 * Compact panel: a quiet, draggable summary of active runs. Always-on-top
 * only when the user enables it in Settings.
 */
export function Widget() {
  const init = useApp((s) => s.init);
  const runs = useApp(useShallow(selectActiveRuns));
  const connected = useApp((s) => s.connected);
  const now = useNow();
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    void init();
  }, [init]);

  const working = runs.filter((r) => r.state !== "waiting_for_user").length;
  const waiting = runs.length - working;
  const summary =
    runs.length === 0
      ? "No active runs"
      : [working && `${working} working`, waiting && `${waiting} waiting for you`].filter(Boolean).join(" · ");

  return (
    <div className="h-full p-1.5">
      <div className="flex h-full flex-col overflow-hidden rounded-[12px] border border-line bg-surface text-fg shadow-soft">
        <div data-tauri-drag-region className="flex h-9 shrink-0 items-center gap-2 border-b border-line px-3">
          <StatusDot state={!connected ? "unknown" : runs.length ? "running" : "completed"} />
          <span data-tauri-drag-region className="flex-1 truncate text-[12.5px] font-medium">
            {connected ? summary : "Collector offline"}
          </span>
          <button
            type="button"
            aria-label={collapsed ? "Expand" : "Collapse"}
            onClick={() => setCollapsed((c) => !c)}
            className="rounded p-1 text-faint hover:bg-surface-2 hover:text-fg"
          >
            <ChevronDown size={13} className={cn("transition-transform", collapsed && "-rotate-90")} />
          </button>
          <button
            type="button"
            aria-label="Hide compact panel"
            onClick={() => native().then((a) => a.setWidgetVisible(false))}
            className="rounded p-1 text-faint hover:bg-surface-2 hover:text-fg"
          >
            <X size={13} />
          </button>
        </div>
        {!collapsed && (
          <ul className="scroll-quiet flex-1 divide-y divide-line overflow-y-auto">
            {runs.map((r) => {
              const eta = etaText(r);
              return (
                <li key={r.id} className="px-3 py-2">
                  <div className="flex items-center justify-between gap-2 text-[12px] text-muted">
                    <span className="truncate font-medium text-fg-2">
                      {AGENT_NAMES[r.agent]}
                      {r.repositoryName && <span className="font-normal text-faint"> · {r.repositoryName}</span>}
                    </span>
                    <span className="tabular">{formatClock(liveElapsedMs(r, now))}</span>
                  </div>
                  <p className={cn("mt-0.5 text-[13px]", eta.kind === "waiting" ? "text-warn" : "text-fg")}>
                    {eta.kind === "waiting" ? "Waiting for you" : eta.headline}
                    {eta.kind === "estimate" && <span className="text-muted"> remaining</span>}
                  </p>
                </li>
              );
            })}
            {runs.length === 0 && <li className="px-3 py-3 text-[12.5px] text-muted">Runs appear here while agents work.</li>}
          </ul>
        )}
      </div>
    </div>
  );
}
