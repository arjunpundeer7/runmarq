import type { Diagnostics, LatencyStats } from "@runmarq/contracts";
import { useEffect, useState } from "react";
import { Button, Kbd, Modal, Notice, PageHeader, Section, Select, Spinner, Toggle } from "../components/controls";
import { useAsync } from "../hooks/useAsync";
import { errorMessage, native } from "../services/native";
import { useApp } from "../stores/app";

function latency(l: LatencyStats): string {
  if (l.p50Ms == null || l.p95Ms == null) return "No samples yet";
  return `p50 ${l.p50Ms.toFixed(1)} ms · p95 ${l.p95Ms.toFixed(1)} ms (${l.samples})`;
}

function DiagnosticsPanel() {
  const info = useApp((s) => s.info);
  const refreshInfo = useApp((s) => s.refreshInfo);
  const notify = useApp((s) => s.notify);
  const diag = useAsync<Diagnostics>(() => native().then((a) => a.diagnostics()), []);
  const [restarting, setRestarting] = useState(false);

  const restart = async () => {
    setRestarting(true);
    try {
      await (await native()).restartCollector();
      await refreshInfo();
      diag.reload();
      notify("Collector restarted.");
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      setRestarting(false);
    }
  };

  const d = diag.data;
  return (
    <div className="space-y-3 text-[13px]">
      <dl className="grid grid-cols-[180px_1fr] gap-x-4 gap-y-1.5">
        <dt className="text-muted">App version</dt>
        <dd>{info?.version ?? "—"}</dd>
        <dt className="text-muted">Collector</dt>
        <dd>
          {info?.collector.running ? `Running · pid ${info.collector.pid} · 127.0.0.1:${info.collector.port}` : "Not running"}
        </dd>
        {d && (
          <>
            <dt className="text-muted">Hook processing</dt>
            <dd className="tabular">{latency(d.hookLatency)}</dd>
            <dt className="text-muted">Event ingestion</dt>
            <dd className="tabular">{latency(d.ingestLatency)}</dd>
            <dt className="text-muted">Forecasting</dt>
            <dd className="tabular">{latency(d.forecastLatency)}</dd>
            <dt className="text-muted">Database writes</dt>
            <dd className="tabular">{latency(d.dbWriteLatency)}</dd>
            <dt className="text-muted">Model training</dt>
            <dd className="tabular">{latency(d.trainingLatency)}</dd>
            <dt className="text-muted">Events</dt>
            <dd className="tabular">
              {d.eventsReceived} received · {d.eventsDuplicate} duplicate · {d.eventsRejected} rejected · {d.spooledEventsReplayed}{" "}
              replayed from spool
            </dd>
            <dt className="text-muted">Pending in spool</dt>
            <dd className="tabular">{d.spoolPending}</dd>
            <dt className="text-muted">Database size</dt>
            <dd className="tabular">{(d.databaseBytes / 1_048_576).toFixed(1)} MB</dd>
          </>
        )}
        <dt className="text-muted">Data folder</dt>
        <dd className="selectable break-all font-mono text-[12px]">{info?.dataDir ?? "—"}</dd>
        <dt className="text-muted">Hook executable</dt>
        <dd className="selectable break-all font-mono text-[12px]">{info?.hookExecutable ?? "—"}</dd>
      </dl>
      {d && d.recentErrors.length > 0 && (
        <Notice tone="warn">
          <p className="mb-1 font-medium">Recent problems</p>
          <ul className="selectable font-mono text-[11.5px]">
            {d.recentErrors.map((e) => (
              <li key={e}>{e}</li>
            ))}
          </ul>
        </Notice>
      )}
      <div className="flex gap-2 pt-1">
        <Button size="sm" onClick={restart} disabled={restarting}>
          {restarting ? <Spinner /> : "Restart collector"}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => native().then((a) => a.openPath("logs"))}>
          Open logs
        </Button>
        <Button size="sm" variant="ghost" onClick={diag.reload}>
          Refresh
        </Button>
      </div>
      <p className="text-[12px] text-faint">
        The same report is available in a terminal with <Kbd>runmarq doctor</Kbd>.
      </p>
    </div>
  );
}

export function Settings() {
  const prefs = useApp((s) => s.preferences);
  const save = useApp((s) => s.savePreferences);
  const notify = useApp((s) => s.notify);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [terminal, setTerminal] = useState<boolean | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    native()
      .then((a) => a.getAutostart())
      .then(setAutostart)
      .catch(() => setAutostart(null));
    native()
      .then((a) => a.getTerminalCommand())
      .then(setTerminal)
      .catch(() => setTerminal(null));
  }, []);

  if (!prefs) {
    return (
      <>
        <PageHeader title="Settings" />
        <Notice tone="warn">Settings are stored by the collector, which is not reachable right now.</Notice>
        <div className="mt-8">
          <Section title="Diagnostics">
            <DiagnosticsPanel />
          </Section>
        </div>
      </>
    );
  }

  const deleteAll = async () => {
    try {
      const n = await (await native()).deleteHistory(null);
      notify(`Deleted ${n} runs.`);
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      setConfirmDelete(false);
    }
  };

  const notificationsOff = !prefs.notificationsEnabled || prefs.quietMode;

  return (
    <>
      <PageHeader title="Settings" />

      <Section title="Privacy" description="Everything stays on this computer. Prompts, source code and command output are never stored.">
        <Toggle
          label="Store short task titles"
          description="Keeps the first line of each prompt (up to 80 characters, with anything that looks like a secret removed) so runs are recognisable. Turning this off erases stored titles."
          checked={prefs.storeTaskTitles}
          onChange={(v) => save({ storeTaskTitles: v })}
        />
        <Toggle
          label="Store repository folder names"
          description="Shows names like “acme-web”. Full paths are never stored. Turning this off erases stored names."
          checked={prefs.storeRepositoryNames}
          onChange={(v) => save({ storeRepositoryNames: v })}
        />
      </Section>

      <Section title="Estimation signals" description="Extra measurements that make estimates more accurate. All are read and kept on this computer.">
        <Toggle
          label="Read token counts from agent transcripts"
          description="Reads Claude Code and Codex session files for response counts, token counts and context size. Message text is never stored."
          checked={prefs.readTranscripts}
          onChange={(v) => save({ readTranscripts: v })}
        />
        <Toggle
          label="Measure working-tree changes"
          description="Runs git diff --numstat in the repository during a run to count changed files and lines. File names are not stored."
          checked={prefs.sampleGitDiff}
          onChange={(v) => save({ sampleGitDiff: v })}
        />
        <Toggle
          label="Compare prompts with earlier ones"
          description="Keeps a salted, hashed word vector of each prompt so similar earlier runs can inform the estimate. The prompt cannot be read back from it. Turning this off erases stored vectors."
          checked={prefs.similarPrompts}
          onChange={(v) => save({ similarPrompts: v })}
        />
      </Section>

      <Section title="Notifications">
        <Toggle label="Notifications" checked={prefs.notificationsEnabled} onChange={(v) => save({ notificationsEnabled: v })} />
        <Toggle
          label="Quiet mode"
          description="Pause all notifications without changing the choices below."
          checked={prefs.quietMode}
          onChange={(v) => save({ quietMode: v })}
          disabled={!prefs.notificationsEnabled}
        />
        <Toggle label="When a run finishes" checked={prefs.notifyOnFinish} onChange={(v) => save({ notifyOnFinish: v })} disabled={notificationsOff} />
        <Toggle label="When a run fails" checked={prefs.notifyOnFailure} onChange={(v) => save({ notifyOnFailure: v })} disabled={notificationsOff} />
        <Toggle
          label="When an agent needs approval"
          checked={prefs.notifyOnAttention}
          onChange={(v) => save({ notifyOnAttention: v })}
          disabled={notificationsOff}
        />
        <Toggle
          label="When an agent stops reporting"
          description="After 20 minutes without updates during a run."
          checked={prefs.notifyOnDisconnect}
          onChange={(v) => save({ notifyOnDisconnect: v })}
          disabled={notificationsOff}
        />
        <Toggle
          label="When a run looks stuck"
          description="Once per episode, when the same tool fails 3 times in a row, one tool call runs far longer than usual, or a run is well past its estimate with no activity."
          checked={prefs.notifyOnStuck}
          onChange={(v) => save({ notifyOnStuck: v })}
          disabled={notificationsOff}
        />
        <Toggle
          label="Play a sound"
          description="Uses the system notification sound."
          checked={prefs.notificationSound}
          onChange={(v) => save({ notificationSound: v })}
          disabled={notificationsOff}
        />
        <div className="flex items-center justify-between gap-6 py-3">
          <div>
            <p className="text-[13.5px] font-medium">Skip very short runs</p>
            <p className="mt-0.5 text-[12.5px] text-muted">Finish and failure notifications only for runs at least this long.</p>
          </div>
          <Select
            aria-label="Minimum run length for notifications"
            value={prefs.notifyMinDurationSeconds}
            onChange={(e) => save({ notifyMinDurationSeconds: Number(e.target.value) })}
            disabled={notificationsOff}
          >
            {[0, 20, 60, 180, 600].map((s) => (
              <option key={s} value={s}>
                {s === 0 ? "Notify for all runs" : s < 60 ? `${s} seconds` : `${s / 60} minutes`}
              </option>
            ))}
          </Select>
        </div>
      </Section>

      <Section title="General">
        <Toggle
          label="Start runmarq when you log in"
          checked={autostart ?? false}
          disabled={autostart == null}
          onChange={async (v) => {
            try {
              setAutostart(await (await native()).setAutostart(v));
            } catch (e) {
              notify(errorMessage(e), "error");
            }
          }}
        />
        <Toggle
          label="Use the runmarq command in new terminals"
          checked={terminal ?? false}
          disabled={terminal == null}
          onChange={async (v) => {
            try {
              const found = await (await native()).setTerminalCommand(v);
              setTerminal(v);
              if (v && !found) notify("runmarq was added to ~/.local/bin. Add that folder to your PATH to use it.", "info");
            } catch (e) {
              notify(errorMessage(e), "error");
            }
          }}
        />
        <Toggle
          label="Compact panel"
          description="A small window listing active runs. The tray menu shows the same information."
          checked={prefs.compactWidget}
          onChange={async (v) => {
            await save({ compactWidget: v });
            await (await native()).setWidgetVisible(v);
          }}
        />
        <Toggle
          label="Keep compact panel on top"
          checked={prefs.widgetAlwaysOnTop}
          onChange={(v) => save({ widgetAlwaysOnTop: v })}
          disabled={!prefs.compactWidget}
        />
      </Section>

      <Section title="Data">
        <div className="flex items-center justify-between gap-6 py-3">
          <div>
            <p className="text-[13.5px] font-medium">Keep history for</p>
            <p className="mt-0.5 text-[12.5px] text-muted">Older finished runs are deleted automatically.</p>
          </div>
          <Select
            aria-label="History retention"
            value={prefs.retentionDays ?? 0}
            onChange={(e) => save({ retentionDays: Number(e.target.value) || null })}
          >
            <option value={0}>Forever</option>
            <option value={30}>30 days</option>
            <option value={90}>90 days</option>
            <option value={365}>1 year</option>
          </Select>
        </div>
        <div className="flex gap-2 py-3">
          <Button onClick={() => native().then((a) => a.exportCsv()).then((p) => p && notify(`Exported to ${p}`))}>Export CSV</Button>
          <Button variant="ghost" onClick={() => native().then((a) => a.openPath("data"))}>
            Open data folder
          </Button>
          <Button variant="danger" className="ml-auto" onClick={() => setConfirmDelete(true)}>
            Delete history
          </Button>
        </div>
      </Section>

      <Section title="Diagnostics">
        <DiagnosticsPanel />
      </Section>

      <Modal
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title="Delete all run history?"
        description="Finished runs, their events and forecasts are removed permanently. Runs in progress are kept. Estimates return to built-in starting values."
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={deleteAll}>
              Delete history
            </Button>
          </>
        }
      />
    </>
  );
}
