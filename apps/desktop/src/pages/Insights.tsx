import type { AccuracyReport, DurationBucket, ErrorPoint, ModelStatus } from "@runmarq/contracts";
import { formatDuration } from "@runmarq/contracts";
import { EmptyState, Notice, PageHeader, Section, Spinner } from "../components/controls";
import { useAsync } from "../hooks/useAsync";
import { native } from "../services/native";
import { useApp } from "../stores/app";

const pct = (v: number | null | undefined) => (v == null ? "—" : `${Math.round(v * 100)}%`);
const secs = (v: number | null | undefined) => (v == null ? "—" : formatDuration(v * 1000));

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div>
      <dt className="text-[12px] text-muted">{label}</dt>
      <dd className="mt-1 text-[22px] font-medium tracking-[-0.02em] tabular">{value}</dd>
      {note && <dd className="mt-0.5 text-[12px] text-faint">{note}</dd>}
    </div>
  );
}

function Distribution({ buckets }: { buckets: DurationBucket[] }) {
  const usable = buckets.filter((b) => b.medianMs != null);
  const hidden = buckets.length - usable.length;
  if (usable.length === 0) {
    return <p className="text-[13px] text-muted">Each group needs at least 5 finished runs before its typical duration is shown.</p>;
  }
  const max = Math.max(...usable.map((b) => b.p90Ms ?? b.medianMs ?? 0), 1);
  return (
    <>
      <ul className="space-y-3">
        {usable.map((b) => (
          <li key={b.key} className="grid grid-cols-[140px_1fr_120px] items-center gap-4 text-[13px]">
            <span className="truncate text-fg-2">{b.label}</span>
            <span className="relative h-1.5 rounded-full bg-surface-2" aria-hidden="true">
              <span className="absolute inset-y-0 left-0 rounded-full bg-accent/20" style={{ width: `${((b.p90Ms ?? 0) / max) * 100}%` }} />
              <span className="absolute inset-y-0 left-0 rounded-full bg-accent" style={{ width: `${((b.medianMs ?? 0) / max) * 100}%` }} />
            </span>
            <span className="text-right tabular text-muted">
              {formatDuration(b.medianMs)} <span className="text-faint">· {b.runs}</span>
            </span>
          </li>
        ))}
      </ul>
      <p className="mt-3 text-[12px] text-faint">
        Bars show the median active duration; the lighter extension reaches the 90th percentile. Numbers after the dot are run counts.
        {hidden > 0 && ` ${hidden} group${hidden === 1 ? "" : "s"} with fewer than 5 runs not shown.`}
      </p>
    </>
  );
}

function ErrorTable({ rows, first }: { rows: ErrorPoint[]; first: string }) {
  return (
    <table className="w-full text-[13px]">
      <thead>
        <tr className="border-b border-line text-left text-[12px] text-muted">
          <th className="py-2 font-normal">{first}</th>
          <th className="py-2 text-right font-normal">Runs</th>
          <th className="py-2 text-right font-normal">Median error</th>
          <th className="py-2 text-right font-normal">In P10–P90</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.label} className="border-b border-line/60 last:border-0">
            <td className="py-2 text-fg-2">{r.label}</td>
            <td className="py-2 text-right tabular text-muted">{r.runs}</td>
            <td className="py-2 text-right tabular">{secs(r.medianAbsErrorSeconds)}</td>
            <td className="py-2 text-right tabular">{pct(r.coverage)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Accuracy({ a }: { a: AccuracyReport }) {
  if (a.evaluatedRuns < a.minimumRunsForMetrics) {
    return (
      <EmptyState title="Not enough finished runs to measure accuracy yet.">
        runmarq reports prediction quality after {a.minimumRunsForMetrics} finished runs with estimates. So far:{" "}
        <span className="tabular">{a.evaluatedRuns}</span>. Estimates generally become narrower and better calibrated as more of
        your own runs are recorded.
      </EmptyState>
    );
  }
  return (
    <div className="space-y-8">
      <dl className="grid grid-cols-2 gap-6 sm:grid-cols-4">
        <Stat label="Inside P10–P90" value={pct(a.intervalCoverage)} note="Nominal target 80%" />
        <Stat label="Median error" value={secs(a.medianAbsErrorSeconds)} />
        <Stat label="Mean error" value={secs(a.meanAbsErrorSeconds)} />
        <Stat label="Runs evaluated" value={String(a.evaluatedRuns)} />
      </dl>
      <div>
        <h3 className="mb-2 text-[13px] font-medium">By forecast age</h3>
        <ErrorTable rows={a.byForecastAge} first="When the estimate was made" />
      </div>
      {a.byAgent.length > 1 && (
        <div>
          <h3 className="mb-2 text-[13px] font-medium">By agent, at run start</h3>
          <ErrorTable rows={a.byAgent} first="Agent" />
        </div>
      )}
      {a.holdout ? (
        <p className="text-[12.5px] text-muted">
          Temporal holdout: fitted on the oldest {a.holdout.trainRuns} runs and tested on the newest {a.holdout.testRuns}, the initial
          estimate&apos;s median error was {secs(a.holdout.medianAbsErrorSeconds)} and {pct(a.holdout.intervalCoverage)} of outcomes
          fell inside the P10–P90 range.
        </p>
      ) : (
        <p className="text-[12.5px] text-muted">A temporal holdout check appears after 30 finished runs.</p>
      )}
      {a.stability != null && (
        <p className="text-[12.5px] text-muted">
          Forecast stability: consecutive estimates differ by {pct(Math.expm1(a.stability))} on average, after accounting for elapsed
          time.
        </p>
      )}
    </div>
  );
}

function Model({ m }: { m: ModelStatus }) {
  const c = m.calibration;
  return (
    <div className="space-y-4">
      <ul className="divide-y divide-line/60">
        {m.components.map((x) => (
          <li key={x.key} className="flex items-start justify-between gap-6 py-2.5">
            <div>
              <p className="text-[13.5px] font-medium">{x.label}</p>
              <p className="mt-0.5 text-[12.5px] text-muted">{x.detail}</p>
            </div>
            <span className={`shrink-0 text-[12px] ${x.active ? "text-accent" : "text-faint"}`}>{x.active ? "In use" : "Not in use"}</span>
          </li>
        ))}
      </ul>
      <p className="text-[12.5px] text-muted">
        {c.active
          ? `The P10–P90 range is calibrated with conformal prediction on ${c.scores} forecasts of ${c.validationRuns} runs the models had not seen, targeting ${pct(c.targetCoverage)} coverage.`
          : `Range calibration starts after ${c.requiredRuns} finished runs (${m.finishedRuns} so far). Until then, ranges come straight from the models.`}
      </p>
      {m.trainedAt && (
        <p className="text-[12px] text-faint">
          Last refitted {new Date(m.trainedAt).toLocaleString()} · {m.version}
        </p>
      )}
    </div>
  );
}

export function Insights() {
  const tick = useApp((s) => s.finishedTick);
  const ins = useAsync(() => native().then((a) => a.insights()), [tick]);
  return (
    <>
      <PageHeader title="Insights" description="Where agent time goes, and how well runmarq's estimates have held up on your machine." />
      {ins.error ? (
        <Notice tone="warn">Insights are unavailable: {ins.error}</Notice>
      ) : !ins.data ? (
        <Spinner />
      ) : ins.data.totalRuns === 0 ? (
        <EmptyState title="No runs recorded yet.">Insights appear once a connected agent has finished a few runs.</EmptyState>
      ) : (
        <>
          <Section title="Activity">
            <dl className="grid grid-cols-2 gap-6 sm:grid-cols-4">
              <Stat label="Runs recorded" value={String(ins.data.totalRuns)} />
              <Stat label="Finished" value={String(ins.data.finishedRuns)} />
              <Stat
                label="Median run"
                value={formatDuration(ins.data.medianDurationMs)}
                note={ins.data.medianDurationMs == null ? "Needs 3 completed runs" : undefined}
              />
              <Stat label="Waiting for approvals" value={formatDuration(ins.data.totalWaitingMs)} />
            </dl>
          </Section>
          <Section title="Prediction quality" description="Measured against runs that have finished. Cancelled runs are excluded.">
            <Accuracy a={ins.data.accuracy} />
          </Section>
          <Section
            title="Estimation model"
            description="Everything is computed on this computer from your own runs. Models join in as they have enough data to help."
          >
            <Model m={ins.data.model} />
          </Section>
          <Section title="Typical duration by agent">
            <Distribution buckets={ins.data.byAgent} />
          </Section>
          <Section title="Typical duration by task type">
            <Distribution buckets={ins.data.byCategory} />
          </Section>
        </>
      )}
    </>
  );
}
