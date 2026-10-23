import type { IntegrationStatus } from "@runmarq/contracts";
import { brand } from "@runmarq/config";
import { cn, LogoMark } from "@runmarq/ui";
import { Check, Minus } from "lucide-react";
import { useState } from "react";
import { Button, Kbd, Notice, Spinner, Toggle } from "../components/controls";
import { useAsync } from "../hooks/useAsync";
import { native } from "../services/native";
import { useApp } from "../stores/app";
import { SetupDialog, STATE_TEXT } from "./Integrations";

type Step = "welcome" | "detect" | "connect" | "verify" | "ready";
const STEPS: Step[] = ["welcome", "detect", "connect", "verify", "ready"];

function Progress({ step }: { step: Step }) {
  const i = STEPS.indexOf(step);
  return (
    <ol className="flex gap-1.5" aria-label={`Step ${i + 1} of ${STEPS.length}`}>
      {STEPS.map((s, j) => (
        <li key={s} className={cn("h-1 w-8 rounded-full", j <= i ? "bg-fg" : "bg-line-strong")} />
      ))}
    </ol>
  );
}

function AgentList({ list }: { list: IntegrationStatus[] }) {
  return (
    <ul className="divide-y divide-line rounded-[var(--radius-card)] border border-line bg-surface">
      {list.map((s) => (
        <li key={s.agent} className="flex items-center justify-between px-4 py-3 text-[13.5px]">
          <span className="font-medium">{s.displayName}</span>
          <span className={cn("inline-flex items-center gap-1.5", s.detected ? "text-fg-2" : "text-faint")}>
            {s.detected ? <Check size={14} className="text-ok" /> : <Minus size={14} />}
            {s.detected ? `Detected${s.installedVersion ? ` · ${s.installedVersion}` : ""}` : "Not detected"}
          </span>
        </li>
      ))}
    </ul>
  );
}

export function Onboarding() {
  const save = useApp((s) => s.savePreferences);
  const [step, setStep] = useState<Step>("welcome");
  const [setupFor, setSetupFor] = useState<IntegrationStatus | null>(null);
  const [connected, setConnected] = useState<Set<string>>(new Set());
  const list = useAsync(() => native().then((a) => a.integrations(null)), [step]);
  const detected = (list.data ?? []).filter((s) => s.detected);

  // Estimates only help if runmarq is running when an agent is, so starting
  // at login is offered switched on.
  const [startAtLogin, setStartAtLogin] = useState(true);
  const [terminalCommand, setTerminalCommand] = useState(true);

  const finish = () => save({ onboardingCompleted: true });
  const openApp = async () => {
    const api = await native();
    // Settings still offers both switches if the system refuses one here.
    await api.setAutostart(startAtLogin).catch(() => undefined);
    if (terminalCommand) await api.setTerminalCommand(true).catch(() => undefined);
    await finish();
  };

  return (
    <div className="flex h-full items-center justify-center bg-bg px-8">
      <div className="w-full max-w-[520px]">
        <div className="mb-10 flex items-center justify-between">
          <LogoMark size={26} />
          <Progress step={step} />
        </div>

        {step === "welcome" && (
          <section className="rm-rise">
            <h1 className="text-[28px] font-semibold leading-tight tracking-[-0.025em]">Your coding agents, on your time.</h1>
            <p className="mt-3 text-[15px] leading-relaxed text-muted">
              {brand.productName} watches Claude Code, Codex and Cursor through their official hooks, estimates when each run will
              finish, and tells you when it is done or needs you. Everything stays on this computer.
            </p>
            <div className="mt-8 flex gap-2">
              <Button variant="primary" onClick={() => setStep("detect")}>
                Get started
              </Button>
              <Button variant="ghost" onClick={finish}>
                Skip setup
              </Button>
            </div>
          </section>
        )}

        {step === "detect" && (
          <section className="rm-rise">
            <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Agents on this computer</h1>
            <p className="mt-2 text-[14px] text-muted">runmarq looked for supported agents in their standard locations.</p>
            <div className="mt-6">{list.loading && !list.data ? <Spinner /> : <AgentList list={list.data ?? []} />}</div>
            {list.data && detected.length === 0 && (
              <div className="mt-4">
                <Notice>No supported agents were found. You can connect one later from Integrations.</Notice>
              </div>
            )}
            <div className="mt-8 flex gap-2">
              <Button variant="primary" onClick={() => setStep(detected.length ? "connect" : "ready")}>
                Continue
              </Button>
            </div>
          </section>
        )}

        {step === "connect" && (
          <section className="rm-rise">
            <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Connect your agents</h1>
            <p className="mt-2 text-[14px] text-muted">
              For each agent you approve, runmarq adds a small hook entry to its configuration. You will see the exact file and entries
              first, and a backup is kept.
            </p>
            <ul className="mt-6 divide-y divide-line rounded-[var(--radius-card)] border border-line bg-surface">
              {detected.map((s) => {
                const done = connected.has(s.agent) || s.state === "connected" || s.state === "awaiting_first_event";
                return (
                  <li key={s.agent} className="flex items-center justify-between gap-4 px-4 py-3">
                    <div>
                      <p className="text-[13.5px] font-medium">{s.displayName}</p>
                      {s.support === "limited" && <p className="text-[12px] text-muted">Limited: {s.limitations[0]}</p>}
                    </div>
                    {done ? (
                      <span className="inline-flex items-center gap-1.5 text-[13px] text-ok">
                        <Check size={14} /> Configured
                      </span>
                    ) : (
                      <Button size="sm" onClick={() => setSetupFor(s)}>
                        Review and connect
                      </Button>
                    )}
                  </li>
                );
              })}
            </ul>
            <div className="mt-8 flex gap-2">
              <Button variant="primary" onClick={() => setStep("verify")}>
                Continue
              </Button>
            </div>
          </section>
        )}

        {step === "verify" && (
          <section className="rm-rise">
            <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Checking the connection</h1>
            <p className="mt-2 text-[14px] text-muted">
              runmarq verified the configuration. An integration shows as connected once the first event arrives from that agent.
            </p>
            <ul className="mt-6 divide-y divide-line rounded-[var(--radius-card)] border border-line bg-surface">
              {(list.data ?? [])
                .filter((s) => s.state !== "not_detected")
                .map((s) => (
                  <li key={s.agent} className="px-4 py-3">
                    <div className="flex items-center justify-between text-[13.5px]">
                      <span className="font-medium">{s.displayName}</span>
                      <span className={STATE_TEXT[s.state].tone}>{STATE_TEXT[s.state].label}</span>
                    </div>
                    {s.pendingSteps.map((p) => (
                      <p key={p} className="mt-1 text-[12.5px] text-muted">
                        {p}
                      </p>
                    ))}
                  </li>
                ))}
            </ul>
            <div className="mt-8 flex gap-2">
              <Button variant="primary" onClick={() => setStep("ready")}>
                Continue
              </Button>
              <Button variant="ghost" onClick={list.reload}>
                Check again
              </Button>
            </div>
          </section>
        )}

        {step === "ready" && (
          <section className="rm-rise">
            <h1 className="text-[22px] font-semibold tracking-[-0.02em]">Your agent runs will appear here.</h1>
            <p className="mt-2 text-[14px] leading-relaxed text-muted">
              Start a new session in a connected agent and send a prompt. runmarq shows an estimate within a second and refines it as
              the agent works. runmarq lives in the system tray; closing this window keeps it running.
            </p>
            <p className="mt-4 text-[13px] text-muted">
              For Codex, starting it through runmarq also closes runs cleanly when Codex exits: <Kbd>runmarq run -- codex</Kbd>
            </p>
            <div className="mt-6 divide-y divide-line rounded-[var(--radius-card)] border border-line bg-surface px-4">
              <Toggle label="Start runmarq when you log in" checked={startAtLogin} onChange={setStartAtLogin} />
              <Toggle label="Use the runmarq command in new terminals" checked={terminalCommand} onChange={setTerminalCommand} />
            </div>
            <div className="mt-8">
              <Button variant="primary" onClick={openApp}>
                Open runmarq
              </Button>
            </div>
          </section>
        )}
      </div>

      <SetupDialog
        status={setupFor}
        projectRoot={null}
        onClose={() => setSetupFor(null)}
        onDone={(s) => {
          setConnected((c) => new Set(c).add(s.agent));
          setSetupFor(null);
          list.reload();
        }}
      />
    </div>
  );
}
