import type { Metadata } from "next";
import { docMetadata } from "@/content/docs";
import Link from "next/link";
import { CodeBlock } from "@/components/CodeBlock";
import { PageTitle, Prose } from "@/components/Prose";
import { buildSteps } from "@/lib/site";

export const metadata: Metadata = docMetadata("/docs/getting-started");

export default function GettingStarted() {
  return (
    <>
      <PageTitle title="Getting started" />
      <Prose>
        <h2>What you need</h2>
        <ul>
          <li>Rust (stable toolchain) and Node.js 20.9 or later with pnpm 9.</li>
          <li>
            The platform prerequisites for Tauri apps: Microsoft C++ Build Tools and WebView2 on Windows, Xcode Command Line Tools on
            macOS, or WebKitGTK and the usual build packages on Linux.
          </li>
          <li>At least one supported agent: Claude Code, Codex 0.130 or later, or Cursor.</li>
        </ul>

        <h2>Build from source</h2>
        <p>
          Until the first release there are no installers; the <Link href="/download">download page</Link> will list them. Get the
          code and build it:
        </p>
        <CodeBlock>{buildSteps}</CodeBlock>
        <p>
          <code>pnpm install</code> installs the JavaScript workspace, <code>cargo build</code> builds the <code>runmarq</code> command,
          and <code>pnpm build:desktop</code> bundles the desktop app with the command included. You can also use only the command-line tool; the desktop app is optional.
        </p>

        <h2>Connect an agent</h2>
        <p>
          The desktop app walks you through this on first launch: it detects installed agents, shows the exact file and entries it will
          add, and asks before writing. From a terminal, the same steps are:
        </p>
        <CodeBlock>{`runmarq collector start
runmarq integrations setup claude
runmarq integrations verify`}</CodeBlock>
        <p>
          Setup prints the change and asks for confirmation. Add <code>--dry-run</code> to only preview, or <code>--project &lt;dir&gt;</code>{" "}
          to configure a single project instead of every project. Replace <code>claude</code> with <code>codex</code> or{" "}
          <code>cursor</code> for the other agents.
        </p>
        <p>
          Hooks call a copy of <code>runmarq</code> kept in runmarq&apos;s data folder, so moving or updating the app does not break them.
          Existing hooks and settings are preserved and a backup is written before every change.
        </p>

        <h2>See your first estimate</h2>
        <p>
          Start a new session in the connected agent and send a prompt. The run appears in the desktop app and in{" "}
          <code>runmarq status</code> within a second, with an estimate that is refined as the agent works. The integration changes from
          “waiting for first event” to “connected” once that first event arrives.
        </p>
        <CodeBlock>{`runmarq status
runmarq history`}</CodeBlock>
        <p>
          Early estimates are deliberately wide, because runmarq starts from conservative built-in values. They narrow as your own runs
          are recorded. <Link href="/docs/eta-explained">How estimates work</Link> explains why.
        </p>
        <p>
          More of the estimator switches on as history builds up: a phase simulation after ten runs with tool activity, a range calibrated
          to your machine after about 34 finished runs, and a learned model after 100. Insights → Estimation model shows which models are
          in use and what each one is waiting for.
        </p>

        <h2>Choose what is measured</h2>
        <p>
          Besides hook events, runmarq reads token counts from Claude Code and Codex transcripts, measures working-tree changes with git,
          and compares each prompt with earlier ones through a salted word vector. Each can be turned off in Settings → Estimation signals;
          estimates keep working without them, with less to go on.
        </p>

        <h2>Use it from the terminal</h2>
        <p>
          The desktop app makes the <code>runmarq</code> command available in new terminals during setup, or later with Settings → Use
          the runmarq command in new terminals.
        </p>
        <p>
          <code>runmarq wait</code> returns when the run in progress ends, with exit code 0 only if it finished, so you can chain your next
          step onto it. <code>runmarq statusline</code> prints one line for your prompt, tmux or Claude Code&apos;s status line, and{" "}
          <code>runmarq eval</code> shows how the estimates have held up on your own runs.
        </p>
        <CodeBlock>{`runmarq wait && npm run deploy
runmarq statusline
runmarq eval`}</CodeBlock>
        <p>
          <Link href="/docs/command-line">Command line reference</Link>
        </p>

        <h2>Codex: start it through runmarq</h2>
        <p>
          Codex has no hook for “the session ended”. Starting Codex through runmarq lets it close runs cleanly when the process exits.
          Your terminal, keystrokes and exit code are passed through unchanged.
        </p>
        <CodeBlock>{`runmarq run -- codex`}</CodeBlock>

        <h2>Check everything</h2>
        <CodeBlock>{`runmarq doctor`}</CodeBlock>
        <p>
          Doctor reports the collector, hook latency, each integration and any pending step, and exits with a nonzero status if something
          needs attention. Status, doctor, history and the integration commands accept <code>--json</code> for scripting.
        </p>

        <h2>Remove runmarq</h2>
        <CodeBlock>{`runmarq forget
runmarq integrations remove claude
runmarq collector stop`}</CodeBlock>
        <p>
          <code>runmarq forget</code> deletes your history and needs the collector running, so it comes first; skip it to keep your
          history. Removing an integration deletes only the entries runmarq added. In the desktop app, also turn off Settings → Use the
          runmarq command in new terminals. To remove everything, delete the data folder afterwards: <code>%LOCALAPPDATA%\runmarq</code>{" "}
          on Windows, <code>~/Library/Application Support/runmarq</code> on macOS, or <code>~/.local/share/runmarq</code> on Linux.
        </p>
      </Prose>
    </>
  );
}
