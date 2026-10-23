import type { AgentSource, HistoryQuery, RunState, RunView } from "@runmarq/contracts";
import { AGENT_NAMES, STATE_LABELS } from "@runmarq/contracts";
import { Download, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button, EmptyState, Notice, PageHeader, Select, Spinner } from "../components/controls";
import { RunTable } from "../components/RunTable";
import { useAsync } from "../hooks/useAsync";
import { errorMessage, native } from "../services/native";
import { useApp } from "../stores/app";

const PAGE = 50;
const RANGES = { all: null, day: 1, week: 7, month: 30 } as const;
type RangeKey = keyof typeof RANGES;

export function History() {
  const finishedTick = useApp((s) => s.finishedTick);
  const notify = useApp((s) => s.notify);
  const [agent, setAgent] = useState<AgentSource | "">("");
  const [state, setState] = useState<RunState | "">("");
  const [repo, setRepo] = useState("");
  const [range, setRange] = useState<RangeKey>("all");
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [extra, setExtra] = useState<RunView[]>([]);
  const [loadingMore, setLoadingMore] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search.trim()), 250);
    return () => clearTimeout(t);
  }, [search]);

  const query = useMemo<HistoryQuery>(
    () => ({
      limit: PAGE,
      agent: agent || undefined,
      state: state || undefined,
      repositoryId: repo || undefined,
      search: debounced || undefined,
    }),
    [agent, state, repo, debounced],
  );
  // The date cutoff is computed when the request is made, not during render.
  const withRange = (q: HistoryQuery): HistoryQuery => {
    const days = RANGES[range];
    return days ? { ...q, from: new Date(Date.now() - days * 86_400_000).toISOString() } : q;
  };

  const page = useAsync(() => native().then((a) => a.history(withRange(query))), [query, range, finishedTick]);
  const repos = useAsync(() => native().then((a) => a.repositories()), [finishedTick]);
  // Paging restarts whenever the filters change.
  const [pagingKey, setPagingKey] = useState("");
  const key = JSON.stringify([query, range, finishedTick]);
  if (key !== pagingKey) {
    setPagingKey(key);
    setExtra([]);
  }

  const runs = [...(page.data?.runs ?? []), ...extra];
  const total = page.data?.total ?? 0;
  const filtered = agent || state || repo || range !== "all" || debounced;

  const loadMore = async () => {
    setLoadingMore(true);
    try {
      const more = await (await native()).history(withRange({ ...query, offset: runs.length }));
      setExtra((e) => [...e, ...more.runs]);
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      setLoadingMore(false);
    }
  };

  const exportCsv = async () => {
    try {
      const path = await (await native()).exportCsv();
      if (path) notify(`Exported to ${path}`);
    } catch (e) {
      notify(`Export failed: ${errorMessage(e)}`, "error");
    }
  };

  return (
    <>
      <PageHeader
        title="History"
        description="Every run runmarq has observed, with its duration and the estimate made when it started."
        actions={
          <Button onClick={exportCsv} disabled={total === 0}>
            <Download size={14} /> Export CSV
          </Button>
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <label className="relative">
          <span className="sr-only">Search</span>
          <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search titles, repositories"
            className="h-8 w-56 rounded-lg border border-line-strong bg-surface pl-8 pr-2.5 text-[13px] outline-none placeholder:text-faint focus-visible:border-accent"
          />
        </label>
        <Select aria-label="Agent" value={agent} onChange={(e) => setAgent(e.target.value as AgentSource | "")}>
          <option value="">All agents</option>
          {(Object.keys(AGENT_NAMES) as AgentSource[]).map((a) => (
            <option key={a} value={a}>
              {AGENT_NAMES[a]}
            </option>
          ))}
        </Select>
        <Select aria-label="Status" value={state} onChange={(e) => setState(e.target.value as RunState | "")}>
          <option value="">Any status</option>
          {(["completed", "failed", "cancelled", "unknown", "running", "waiting_for_user"] as RunState[]).map((s) => (
            <option key={s} value={s}>
              {STATE_LABELS[s]}
            </option>
          ))}
        </Select>
        <Select aria-label="Repository" value={repo} onChange={(e) => setRepo(e.target.value)}>
          <option value="">All repositories</option>
          {(repos.data ?? []).map((r) => (
            <option key={r.id} value={r.id}>
              {r.displayName ?? `Unnamed (${r.id.slice(5, 11)})`}
            </option>
          ))}
        </Select>
        <Select aria-label="Date range" value={range} onChange={(e) => setRange(e.target.value as RangeKey)}>
          <option value="all">All time</option>
          <option value="day">Last 24 hours</option>
          <option value="week">Last 7 days</option>
          <option value="month">Last 30 days</option>
        </Select>
      </div>

      {page.error ? (
        <Notice tone="warn">History is unavailable: {page.error}</Notice>
      ) : page.loading && !page.data ? (
        <Spinner />
      ) : runs.length === 0 ? (
        <EmptyState title={filtered ? "No runs match these filters." : "No runs recorded yet."}>
          {filtered ? "Try a wider date range or clear a filter." : "Runs appear here once a connected agent finishes its first task."}
        </EmptyState>
      ) : (
        <>
          <RunTable runs={runs} />
          <div className="mt-3 flex items-center justify-between text-[12.5px] text-muted">
            <span className="tabular">
              {runs.length} of {total} runs
            </span>
            {runs.length < total && (
              <Button size="sm" onClick={loadMore} disabled={loadingMore}>
                {loadingMore ? <Spinner /> : "Load more"}
              </Button>
            )}
          </div>
          <p className="mt-6 text-[12px] text-faint">
            Initial estimates show their likely range. Amber marks runs that finished outside even the full P10–P90 range. Search covers
            task titles only when title storage is on in Settings.
          </p>
        </>
      )}
    </>
  );
}
