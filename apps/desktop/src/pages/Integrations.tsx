import type { AgentCapabilities, IntegrationState, IntegrationStatus } from "@runmarq/contracts";
import { CAPABILITY_LABELS, SUPPORT_LABELS } from "@runmarq/contracts";
import { cn } from "@runmarq/ui";
import { Check, ChevronDown, FolderOpen, Minus } from "lucide-react";
import { useState } from "react";
import { Button, Kbd, Modal, Notice, PageHeader, Spinner } from "../components/controls";
import { useAsync } from "../hooks/useAsync";
import type { InstallPlan } from "../services/native";
import { errorMessage, native } from "../services/native";
import { useApp } from "../stores/app";

export const STATE_TEXT: Record<IntegrationState, { label: string; tone: string }> = {
  not_detected: { label: "Not detected", tone: "text-faint" },
  not_connected: { label: "Not connected", tone: "text-muted" },
  awaiting_first_event: { label: "Waiting for first event", tone: "text-warn" },
  connected: { label: "Connected", tone: "text-ok" },
  limited: { label: "Limited", tone: "text-warn" },
  error: { label: "Needs attention", tone: "text-danger" },
};

export function SetupDialog({
  status,
  projectRoot,
  onClose,
  onDone,
}: {
  status: IntegrationStatus | null;
  projectRoot: string | null;
  onClose: () => void;
  onDone: (s: IntegrationStatus) => void;
}) {
  const notify = useApp((s) => s.notify);
  const plan = useAsync<InstallPlan | null>(
    () => (status ? native().then((a) => a.integrationPlan(status.agent, projectRoot)) : Promise.resolve(null)),
    [status?.agent, projectRoot],
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!status) return null;

  const approve = async () => {
    setBusy(true);
    setError(null);
    try {
      const out = await (await native()).integrationInstall(status.agent, projectRoot);
      notify(out.result.changed ? `${status.displayName} hooks installed.` : `${status.displayName} was already set up.`);
      onDone(out.status);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      wide
      onOpenChange={(o) => !o && onClose()}
      title={`Connect ${status.displayName}`}
      description="runmarq adds a small hook entry to the agent's configuration. Nothing else in the file changes, and a backup is written first."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={approve} disabled={busy || !plan.data}>
            {busy ? <Spinner className="border-t-bg" /> : plan.data?.alreadyInstalled ? "Re-apply" : "Approve and connect"}
          </Button>
        </>
      }
    >
      {plan.loading ? (
        <Spinner />
      ) : plan.error ? (
        <Notice tone="danger">{plan.error}</Notice>
      ) : plan.data ? (
        <div className="space-y-4 text-[13px]">
          <dl className="grid grid-cols-[88px_1fr] gap-x-3 gap-y-2">
            <dt className="text-muted">File</dt>
            <dd className="selectable break-all font-mono text-[12px]">
              {plan.data.configPath}
              {!plan.data.fileExists && <span className="ml-2 font-sans text-muted">(will be created)</span>}
            </dd>
            <dt className="text-muted">Command</dt>
            <dd className="selectable break-all font-mono text-[12px]">{plan.data.command}</dd>
            <dt className="text-muted">Events</dt>
            <dd className="text-fg-2">{plan.data.events.join(", ")}</dd>
          </dl>
          <details className="rounded-lg border border-line">
            <summary className="cursor-default px-3 py-2 text-[12.5px] text-muted">Exact entries runmarq adds</summary>
            <pre className="selectable scroll-quiet max-h-56 overflow-auto border-t border-line bg-surface-2 p-3 font-mono text-[11.5px] leading-relaxed">
              {JSON.stringify(plan.data.preview, null, 2)}
            </pre>
          </details>
          <ul className="space-y-1 text-[12.5px] text-muted">
            {plan.data.notes.map((n) => (
              <li key={n}>· {n}</li>
            ))}
          </ul>
          {error && <Notice tone="danger">{error}</Notice>}
        </div>
      ) : null}
    </Modal>
  );
}

function Capabilities({ caps }: { caps: AgentCapabilities }) {
  return (
    <ul className="grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-3">
      {(Object.keys(CAPABILITY_LABELS) as Array<keyof AgentCapabilities>).map((k) => (
        <li key={k} className={cn("flex items-center gap-1.5 text-[12.5px]", caps[k] ? "text-fg-2" : "text-faint")}>
          {caps[k] ? <Check size={13} className="text-ok" /> : <Minus size={13} />}
          {CAPABILITY_LABELS[k]}
        </li>
      ))}
    </ul>
  );
}

function IntegrationRow({
  s,
  onSetup,
  onRemove,
}: {
  s: IntegrationStatus;
  onSetup: () => void;
  onRemove: () => void;
}) {
  const [open, setOpen] = useState(false);
  const st = STATE_TEXT[s.state];
  const configured = s.state === "connected" || s.state === "awaiting_first_event" || s.state === "error";
  return (
    <li className="border-b border-line py-5 last:border-b-0">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="flex items-baseline gap-3">
            <h2 className="text-[15px] font-semibold tracking-[-0.01em]">{s.displayName}</h2>
            <span className={cn("text-[13px] font-medium", st.tone)}>{st.label}</span>
            <span className="text-[12px] text-faint">{SUPPORT_LABELS[s.support]} support</span>
          </div>
          <p className="mt-1 text-[12.5px] text-muted">
            {s.detected
              ? `Detected${s.installedVersion ? `, version ${s.installedVersion}` : ""}`
              : "Not found on this machine. You can still set it up before installing it."}
          </p>
        </div>
        <div className="flex shrink-0 gap-2">
          {configured && (
            <Button variant="ghost" size="sm" onClick={onRemove}>
              Remove
            </Button>
          )}
          <Button size="sm" variant={configured ? "secondary" : "primary"} onClick={onSetup}>
            {configured ? "Re-run setup" : "Set up"}
          </Button>
        </div>
      </div>

      {s.pendingSteps.length > 0 && (
        <div className="mt-3">
          <Notice tone="warn">
            {s.pendingSteps.map((p) => (
              <p key={p}>{p}</p>
            ))}
          </Notice>
        </div>
      )}

      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="mt-3 inline-flex items-center gap-1 text-[12.5px] text-muted hover:text-fg"
      >
        <ChevronDown size={13} className={cn("transition-transform", open && "rotate-180")} />
        Capabilities and diagnostics
      </button>
      {open && (
        <div className="rm-rise mt-3 space-y-4 rounded-lg bg-surface-2/60 p-4">
          <Capabilities caps={s.capabilities} />
          {s.limitations.length > 0 && (
            <ul className="space-y-1 text-[12.5px] text-muted">
              {s.limitations.map((l) => (
                <li key={l}>· {l}</li>
              ))}
            </ul>
          )}
          <ul className="space-y-1 border-t border-line pt-3 text-[12.5px]">
            {s.checks.map((c) => (
              <li key={c.label} className="flex gap-2">
                <span className={c.ok ? "text-ok" : "text-faint"}>{c.ok ? <Check size={13} /> : <Minus size={13} />}</span>
                <span className="text-fg-2">{c.label}</span>
                {c.detail && <span className="selectable truncate text-muted">{c.detail}</span>}
              </li>
            ))}
          </ul>
          <p className="text-[12px] text-muted">
            Configuration: <span className="selectable font-mono">{s.configLocations.join(", ")}</span>
          </p>
        </div>
      )}
    </li>
  );
}

export function Integrations() {
  const notify = useApp((s) => s.notify);
  const [projectRoot, setProjectRoot] = useState<string | null>(null);
  const list = useAsync(() => native().then((a) => a.integrations(projectRoot)), [projectRoot]);
  const [setupFor, setSetupFor] = useState<IntegrationStatus | null>(null);
  const [removeFor, setRemoveFor] = useState<IntegrationStatus | null>(null);

  const remove = async () => {
    if (!removeFor) return;
    try {
      const r = await (await native()).integrationRemove(removeFor.agent, projectRoot);
      notify(r.removedEntries ? `Removed ${r.removedEntries} hook entries.` : "No runmarq hooks were found.");
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      setRemoveFor(null);
    }
  };

  return (
    <>
      <PageHeader
        title="Integrations"
        description="runmarq connects through each agent's official hook interface. Only capabilities the agent exposes are used."
        actions={
          <Button variant="ghost" onClick={list.reload}>
            Refresh
          </Button>
        }
      />
      <div className="mb-2 flex items-center gap-3 text-[13px]">
        <span className="text-muted">Scope</span>
        <div className="inline-flex rounded-lg border border-line-strong p-0.5">
          <button
            type="button"
            onClick={() => setProjectRoot(null)}
            className={cn("rounded-md px-2.5 py-1", projectRoot == null ? "bg-surface-2 font-medium" : "text-muted")}
          >
            All projects
          </button>
          <button
            type="button"
            onClick={async () => {
              const dir = await (await native()).pickDirectory();
              if (dir) setProjectRoot(dir);
            }}
            className={cn("inline-flex items-center gap-1.5 rounded-md px-2.5 py-1", projectRoot ? "bg-surface-2 font-medium" : "text-muted")}
          >
            <FolderOpen size={13} />
            {projectRoot ? "One project" : "Choose project…"}
          </button>
        </div>
        {projectRoot && <Kbd>{projectRoot}</Kbd>}
      </div>

      {list.error ? (
        <Notice tone="warn">{list.error}</Notice>
      ) : list.loading && !list.data ? (
        <div className="py-6">
          <Spinner />
        </div>
      ) : (
        <ul>
          {(list.data ?? []).map((s) => (
            <IntegrationRow key={s.agent} s={s} onSetup={() => setSetupFor(s)} onRemove={() => setRemoveFor(s)} />
          ))}
        </ul>
      )}

      <p className="mt-6 text-[12.5px] text-muted">
        “Connected” appears only after runmarq has received an event from the agent. Until then a configured integration shows
        “Waiting for first event”.
      </p>

      <SetupDialog
        status={setupFor}
        projectRoot={projectRoot}
        onClose={() => setSetupFor(null)}
        onDone={() => {
          setSetupFor(null);
          list.reload();
        }}
      />
      <Modal
        open={removeFor != null}
        onOpenChange={(o) => !o && setRemoveFor(null)}
        title={`Remove ${removeFor?.displayName ?? ""} integration?`}
        description="Only the hook entries runmarq added are removed. Your other settings and hooks stay as they are, and a backup is written first."
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoveFor(null)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={remove}>
              Remove
            </Button>
          </>
        }
      />
    </>
  );
}
