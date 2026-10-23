// Per-agent documentation. Capability tables come from the generated
// manifest; this file adds the prose. Facts here were checked against the
// vendor documentation and the installed versions listed.
import type { AgentSource } from "@runmarq/contracts";
import type { ReactNode } from "react";
import { CodeBlock } from "@/components/CodeBlock";

export interface AgentDoc {
  slug: string;
  id: AgentSource;
  testedWith: string;
  body: ReactNode;
}

export const AGENT_DOCS: AgentDoc[] = [
  {
    slug: "claude-code",
    id: "claude_code",
    testedWith: "Claude Code 2.1.143, with live runs",
    body: (
      <>
        <h2>How it connects</h2>
        <p>
          runmarq adds command hooks to Claude Code&apos;s settings: <code>~/.claude/settings.json</code> for all projects, or{" "}
          <code>.claude/settings.local.json</code> inside one project. The project file is the personal, git-ignored one, so setup never
          edits settings your team shares. <code>CLAUDE_CONFIG_DIR</code> is respected.
        </p>
        <CodeBlock>{`runmarq integrations setup claude
runmarq integrations setup claude --project ./my-repo`}</CodeBlock>
        <p>
          Hooks are registered for <code>SessionStart</code>, <code>UserPromptSubmit</code>, <code>PreToolUse</code>,{" "}
          <code>PostToolUse</code>, <code>PostToolUseFailure</code>, <code>PermissionRequest</code>, <code>Notification</code>,{" "}
          <code>SubagentStart</code>, <code>SubagentStop</code>, <code>PreCompact</code>, <code>Stop</code>, <code>StopFailure</code> and{" "}
          <code>SessionEnd</code>. Each runs <code>runmarq emit --source claude_code</code>, prints nothing and exits 0, so it cannot block a
          tool call or add text to your conversation.
        </p>
        <h2>What happens to each event</h2>
        <ul>
          <li>A submitted prompt starts a run. The prompt is classified in the hook process and then discarded.</li>
          <li>
            A prompt that arrives while Claude is still working continues the same run: a message you typed during the turn, or Claude
            Code reporting that a background command finished. The first prompt names the task.
          </li>
          <li>Tool events set the current activity. Shell commands are inspected in memory only to tell tests, builds and installs apart.</li>
          <li>Permission requests and permission notifications pause the estimate and can notify you.</li>
          <li>
            <code>Stop</code> ends the run as finished; <code>StopFailure</code> ends it as failed.
          </li>
          <li>Claude&apos;s task list, or its older to-do list, provides plan progress. Only the counts are kept.</li>
          <li>
            Each event names the session transcript on your computer. runmarq reads the numbers in it (responses, token counts, context
            size, compactions and effort level) and keeps neither its text nor its location.
          </li>
        </ul>
        <h2>Interrupts</h2>
        <p>
          Claude Code does not fire <code>Stop</code> when you interrupt a turn. runmarq treats a tool failure flagged as an interrupt, an
          idle notification without a preceding stop, or a new prompt after Claude has gone quiet as the end of the run, and marks it
          cancelled at its last activity. An interrupted run is never shown as finished.
        </p>
        <p>
          If Claude Code is closed mid-run, for example with its terminal window, runmarq notices within seconds that its process has
          exited and marks the run cancelled.
        </p>
        <h2>Running inside Cursor</h2>
        <p>
          Cursor can also run hooks from Claude Code settings files. Those payloads are recognised and ignored, so a Cursor run is counted
          once, through the native Cursor integration.
        </p>
      </>
    ),
  },
  {
    slug: "codex",
    id: "codex",
    testedWith: "Codex CLI 0.130.0 hook payloads; not yet run live",
    body: (
      <>
        <h2>How it connects</h2>
        <p>
          Codex 0.130 has stable lifecycle hooks, enabled by default. runmarq writes them to <code>~/.codex/hooks.json</code> (or{" "}
          <code>.codex/hooks.json</code> in a project); it never edits <code>config.toml</code>. <code>CODEX_HOME</code> is respected.
        </p>
        <CodeBlock>{`runmarq integrations setup codex`}</CodeBlock>
        <p>
          <strong>One manual step:</strong> Codex does not run hooks until you trust them. Open Codex, run <code>/hooks</code> and trust
          the runmarq entries. Until the first event arrives, runmarq shows the integration as waiting rather than connected.
        </p>
        <h2>Events used</h2>
        <p>
          <code>SessionStart</code>, <code>UserPromptSubmit</code>, <code>PreToolUse</code>, <code>PermissionRequest</code>,{" "}
          <code>PostToolUse</code>, <code>PreCompact</code> and <code>Stop</code>. Every event after session start carries a turn id,
          which maps one-to-one onto a runmarq run.
        </p>
        <p>
          Events also name the session&apos;s rollout file on your computer. runmarq reads token counts, the context window and the effort
          level from it, and keeps neither its text nor its location.
        </p>
        <h2>Start Codex through runmarq</h2>
        <p>
          Codex 0.130 has no session-end, interrupt or tool-failure hooks. When Codex is started through the wrapper, runmarq learns when
          the process exits and closes any run it left open as cancelled. Failed shell commands are detected from exit codes when Codex
          reports them.
        </p>
        <CodeBlock>{`runmarq run -- codex
runmarq run -- codex exec --json "fix the failing test"`}</CodeBlock>
        <p>
          With <code>codex exec --json</code>, the wrapper also reads Codex&apos;s structured event stream while passing every line through
          unchanged, and suppresses the hooks for that process so nothing is counted twice.
        </p>
        <h2>What is not possible</h2>
        <p>
          Codex sessions already running in an IDE, started without hooks, cannot be observed. runmarq does not read terminal output to
          guess at progress.
        </p>
      </>
    ),
  },
  {
    slug: "cursor",
    id: "cursor",
    testedWith: "Cursor 3.23 hook payloads; not yet run live",
    body: (
      <>
        <h2>How it connects</h2>
        <p>
          runmarq adds Cursor agent hooks to <code>~/.cursor/hooks.json</code>, or <code>.cursor/hooks.json</code> in a project. Project
          hooks run only in trusted workspaces. Cursor reloads the file automatically; restart Cursor if the Hooks panel does not list
          runmarq.
        </p>
        <CodeBlock>{`runmarq integrations setup cursor`}</CodeBlock>
        <h2>An observer-only event set</h2>
        <p>
          Cursor&apos;s pre-execution hooks (<code>preToolUse</code>, <code>beforeShellExecution</code> and others) block an action when
          their output does not match the expected format, and an explicit “allow” may skip your normal approval prompt. A tool that only
          observes should take neither risk, so runmarq registers only <code>sessionStart</code>, <code>beforeSubmitPrompt</code>,{" "}
          <code>postToolUse</code>, <code>postToolUseFailure</code>, <code>subagentStop</code>, <code>stop</code> and{" "}
          <code>sessionEnd</code>.
        </p>
        <p>
          As a result, runmarq sees tools when they finish rather than when they start, and cannot see approval prompts in Cursor. A turn
          Cursor reports as aborted is recorded as cancelled; one it reports as an error is recorded as failed.
        </p>
        <h2>Windows</h2>
        <p>
          Cursor does not document which shell runs hooks on Windows. runmarq writes a command without quotes or spaces (using the short
          form of the path when needed) so it works in both PowerShell and cmd. After setup, send one prompt and confirm the integration
          shows as connected.
        </p>
        <h2>Agent runs only</h2>
        <p>Commands you type yourself in Cursor&apos;s integrated terminal are not agent activity and are not tracked.</p>
      </>
    ),
  },
];
