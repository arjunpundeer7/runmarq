import { RunCard } from "@runmarq/ui";
import { ArrowUpRight } from "lucide-react";
import { Button, EmptyState, Notice, PageHeader, Section, Spinner } from "../components/controls";
import { RunTable } from "../components/RunTable";
import { useAsync } from "../hooks/useAsync";
import { useNow } from "../hooks/useNow";
import { native } from "../services/native";
import { useShallow } from "zustand/react/shallow";
import { selectActiveRuns, useApp } from "../stores/app";

export function CollectorBanner() {
  const connected = useApp((s) => s.connected);
  const error = useApp((s) => s.connectionError);
  const refresh = useApp((s) => s.refreshInfo);
  if (connected) return null;
  return (
    <div className="mb-6">
      <Notice tone="warn">
        <span className="font-medium">The collector is not reachable.</span> Hooks keep working: events are stored on disk and
        replayed when it is back.{" "}
        {error && !/not running/i.test(error) && <span className="opacity-80">({error}) </span>}
        <button type="button" className="underline underline-offset-2" onClick={() => native().then((a) => a.restartCollector()).then(refresh)}>
          Restart collector
        </button>
      </Notice>
    </div>
  );
}

export function Overview() {
  const runs = useApp(useShallow(selectActiveRuns));
  const navigate = useApp((s) => s.navigate);
  const finishedTick = useApp((s) => s.finishedTick);
  const now = useNow();
  const recent = useAsync(() => native().then((a) => a.history({ limit: 6 })), [finishedTick]);
  const finished = recent.data?.runs.filter((r) => !runs.some((a) => a.id === r.id)) ?? [];
  const working = runs.filter((r) => r.state !== "waiting_for_user").length;

  return (
    <>
      <PageHeader
        title="Overview"
        description={
          runs.length === 0
            ? "No agents are running right now."
            : `${working} ${working === 1 ? "agent" : "agents"} working${runs.length > working ? `, ${runs.length - working} waiting for you` : ""}.`
        }
      />
      <CollectorBanner />
      {runs.length === 0 ? (
        <EmptyState title="Your agent runs will appear here.">
          Send a prompt in a connected agent, such as Claude Code. runmarq picks up the run, estimates how long it will take, and
          lets you know when it finishes.
          <div className="mt-4">
            <Button size="sm" onClick={() => navigate({ page: "integrations" })}>
              Manage integrations
            </Button>
          </div>
        </EmptyState>
      ) : (
        <div className="grid gap-4">
          {runs.map((r) => (
            <RunCard
              key={r.id}
              run={r}
              now={now}
              footer={
                <Button variant="ghost" size="sm" onClick={() => navigate({ page: "run", id: r.id })}>
                  Details <ArrowUpRight size={13} />
                </Button>
              }
            />
          ))}
        </div>
      )}

      <div className="mt-10">
        <Section
          title="Recent runs"
          actions={
            <Button variant="ghost" size="sm" onClick={() => navigate({ page: "history" })}>
              View history
            </Button>
          }
        >
          {recent.loading && !recent.data ? (
            <Spinner />
          ) : recent.error ? (
            <p className="text-[13px] text-muted">History is unavailable while the collector is offline.</p>
          ) : finished.length === 0 ? (
            <p className="text-[13px] text-muted">Finished runs will be listed here with their actual duration and the initial estimate.</p>
          ) : (
            <RunTable runs={finished.slice(0, 5)} dense />
          )}
        </Section>
      </div>
    </>
  );
}
