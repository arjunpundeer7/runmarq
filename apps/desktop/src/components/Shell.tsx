import { brand } from "@runmarq/config";
import { cn, StatusDot, Wordmark } from "@runmarq/ui";
import { Activity, BarChart3, Gauge, History, Plug, Settings } from "lucide-react";
import type { ReactNode } from "react";
import type { Route } from "../services/native";
import { selectActiveRuns, useApp } from "../stores/app";

const NAV: Array<{ page: Route["page"]; label: string; icon: ReactNode }> = [
  { page: "overview", label: "Overview", icon: <Activity size={15} strokeWidth={1.75} /> },
  { page: "history", label: "History", icon: <History size={15} strokeWidth={1.75} /> },
  { page: "integrations", label: "Integrations", icon: <Plug size={15} strokeWidth={1.75} /> },
  { page: "insights", label: "Insights", icon: <BarChart3 size={15} strokeWidth={1.75} /> },
  { page: "usage", label: "Usage", icon: <Gauge size={15} strokeWidth={1.75} /> },
  { page: "settings", label: "Settings", icon: <Settings size={15} strokeWidth={1.75} /> },
];

export function Shell({ children }: { children: ReactNode }) {
  const route = useApp((s) => s.route);
  const navigate = useApp((s) => s.navigate);
  const connected = useApp((s) => s.connected);
  const activeCount = useApp((s) => selectActiveRuns(s).length);
  const toast = useApp((s) => s.toast);
  const current = route.page === "run" ? "history" : route.page;

  return (
    <div className="flex h-full bg-bg text-fg">
      <aside className="flex w-[200px] shrink-0 flex-col border-r border-line px-3 pb-4 pt-5">
        <div className="px-2 pb-6">
          <Wordmark name={brand.productName} />
        </div>
        <nav aria-label="Primary" className="flex flex-col gap-0.5">
          {NAV.map((n) => (
            <button
              key={n.page}
              type="button"
              onClick={() => navigate({ page: n.page } as Route)}
              aria-current={current === n.page ? "page" : undefined}
              className={cn(
                "flex h-8 items-center gap-2.5 rounded-lg px-2 text-left text-[13.5px] transition-colors duration-150",
                current === n.page ? "bg-surface-2 font-medium text-fg" : "text-muted hover:bg-surface-2/60 hover:text-fg",
              )}
            >
              <span className={current === n.page ? "text-fg" : "text-faint"}>{n.icon}</span>
              {n.label}
              {n.page === "overview" && activeCount > 0 && (
                <span className="ml-auto rounded-full bg-accent-muted px-1.5 text-[11px] font-medium tabular text-accent-text">
                  {activeCount}
                </span>
              )}
            </button>
          ))}
        </nav>
        <div className="mt-auto px-2 text-[12px] text-muted">
          <span className="inline-flex items-center gap-2">
            <StatusDot state={connected ? "completed" : "unknown"} />
            {connected ? "Collector running" : "Collector offline"}
          </span>
        </div>
      </aside>
      <main className="scroll-quiet min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[920px] px-8 pb-16 pt-8">{children}</div>
      </main>
      {toast && (
        <div
          role="status"
          className={cn(
            "rm-rise fixed bottom-5 right-5 max-w-sm rounded-lg border px-3.5 py-2.5 text-[13px] shadow-soft",
            toast.tone === "error" ? "border-danger/30 bg-danger-muted text-danger" : "border-line bg-surface text-fg-2",
          )}
        >
          {toast.text}
        </div>
      )}
    </div>
  );
}
