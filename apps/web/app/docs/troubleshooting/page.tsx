import type { Metadata } from "next";
import { docMetadata } from "@/content/docs";
import Link from "next/link";
import type { ReactNode } from "react";
import { CodeBlock } from "@/components/CodeBlock";
import { PageTitle, Prose } from "@/components/Prose";

export const metadata: Metadata = docMetadata("/docs/troubleshooting");

const PROBLEMS: Array<{ q: string; a: ReactNode }> = [
  {
    q: "An integration says “waiting for first event”.",
    a: (
      <>
        The configuration is in place but no event has arrived yet. Start a <em>new</em> session in the agent and send a prompt; sessions
        that were already open may not have loaded the new hooks. For Codex, run <code>/hooks</code> inside Codex and trust the runmarq
        entries first.
      </>
    ),
  },
  {
    q: "Setup says it is refusing to modify a file.",
    a: (
      <>
        The agent&apos;s configuration file is not valid JSON, or a hooks entry is not a list. runmarq never rewrites a file it cannot
        parse. Fix or move the file, then run setup again.
      </>
    ),
  },
  {
    q: "The collector is not running.",
    a: (
      <>
        Hooks keep working: events are written to a small spool on disk and replayed with their original times when the collector starts.
        Start it with <code>runmarq collector start</code> or by opening the desktop app.
      </>
    ),
  },
  {
    q: "A run shows “Status unknown”.",
    a: (
      <>
        No events arrived for 20 minutes while the run was working, or runmarq restarted during the run. runmarq does not guess that it
        finished. The run resumes as soon as the agent sends another event. If Claude Code or Codex was closed, runmarq usually sees its
        process exit within seconds and marks the run cancelled instead.
      </>
    ),
  },
  {
    q: "A run I interrupted shows as cancelled.",
    a: <>That is intended. An interrupted run did not finish, so it is never counted as finished or used as an ordinary duration.</>,
  },
  {
    q: "Estimates are very wide.",
    a: (
      <>
        With little local history, estimates rely on conservative built-in values. They narrow as your runs are recorded. See{" "}
        <Link href="/docs/eta-explained">How estimates work</Link>.
      </>
    ),
  },
  {
    q: "Run activity shows no token counts.",
    a: (
      <>
        Token counts come from the session files Claude Code and Codex write on your computer; Cursor does not write one runmarq can read.
        Check that Settings → Estimation signals → “Read token counts from agent transcripts” is on. Counts appear after the agent&apos;s
        first response in a run.
      </>
    ),
  },
  {
    q: "Working-tree changes do not appear.",
    a: (
      <>
        Changes are measured with <code>git diff</code> against the last commit, so the folder the agent works in must be a git repository
        with at least one commit, and <code>git</code> must be on your path. A repository too large to compare within three seconds is
        skipped rather than slowing anything down.
      </>
    ),
  },
  {
    q: "Insights says a model is not in use.",
    a: (
      <>
        That is expected early on. The phase simulation needs ten finished runs with tool activity, range calibration about 34 finished
        runs, and the learned model 100. A model that did not improve forecasts on your recent runs is also left out until it does.
      </>
    ),
  },
  {
    q: "A run says it may be stuck.",
    a: (
      <>
        runmarq saw the same tool fail three times in a row, one tool call run far longer than that command usually takes, or the run go
        well past its estimate with nothing happening for five minutes. It is a prompt to look, not a verdict: the warning clears by
        itself when the agent makes progress. Turn the notification off in Settings → Notifications.
      </>
    ),
  },
  {
    q: "The Usage page does not match my plan's meter.",
    a: (
      <>
        It counts the output tokens your agents wrote into their transcripts on this computer. Input and cached tokens are not
        included, and runmarq does not know your plan&apos;s limits, so treat it as a local tally rather than the vendor&apos;s figure.
      </>
    ),
  },
  {
    q: "No notifications appear.",
    a: (
      <>
        Check Settings → Notifications, including quiet mode and the minimum run length, and your operating system&apos;s notification
        permission for runmarq. Finish and failure notifications are skipped for runs shorter than the minimum.
      </>
    ),
  },
];

export default function Troubleshooting() {
  return (
    <>
      <PageTitle title="Troubleshooting" />
      <Prose>
        <h2>Start with doctor</h2>
        <CodeBlock>{`runmarq doctor`}</CodeBlock>
        <p>
          It reports the collector&apos;s state and latency, spooled events, every integration&apos;s checks and pending steps, and recent
          errors, and says what to do next. It exits with status 1 when something needs attention, and <code>runmarq doctor --json</code>{" "}
          gives the same report for scripts or a bug report.
        </p>
        <h2>Common problems</h2>
        {PROBLEMS.map((p) => (
          <div key={p.q}>
            <h3>{p.q}</h3>
            <p>{p.a}</p>
          </div>
        ))}
        <h2>Where things are</h2>
        <table>
          <tbody>
            <tr>
              <td>Data folder</td>
              <td>
                <code>%LOCALAPPDATA%\runmarq</code>, <code>~/Library/Application Support/runmarq</code> or{" "}
                <code>~/.local/share/runmarq</code>
              </td>
            </tr>
            <tr>
              <td>Logs</td>
              <td>
                <code>logs/</code> in the data folder: one file per day, seven kept, plus <code>hook.log</code>
              </td>
            </tr>
            <tr>
              <td>Configuration backups</td>
              <td>
                <code>backups/</code> in the data folder, one copy per change
              </td>
            </tr>
            <tr>
              <td>Hook executable</td>
              <td>
                <code>bin/runmarq</code> in the data folder
              </td>
            </tr>
          </tbody>
        </table>
        <p>
          Set <code>RUNMARQ_LOG=debug</code> before starting the collector for more detail. Logs contain timing and errors, never prompts or
          file contents. Set <code>RUNMARQ_HOME</code> to use a different data folder.
        </p>
      </Prose>
    </>
  );
}
