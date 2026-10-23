// Every runmarq command, once: the command-line reference renders it in
// full, and the features page and /llms.txt link to each one.
import Link from "next/link";
import type { ReactNode } from "react";

export interface Command {
  usage: string;
  what: ReactNode;
  options?: Array<[string, string]>;
}

/** The words after `runmarq` up to the first argument: "integrations setup". */
export function commandName(usage: string): string {
  const words = usage.split(/\s+/).slice(1);
  const end = words.findIndex((w) => !/^[a-z]+$/.test(w));
  return words.slice(0, end === -1 ? undefined : end).join(" ");
}

/** Anchor of a command on the command-line page. */
export function commandId(usage: string): string {
  return commandName(usage).replace(/ /g, "-");
}

export const COMMANDS: Array<{ group: string; items: Command[] }> = [
  {
    group: "Runs",
    items: [
      {
        usage: "runmarq status",
        what: "The collector and every active run, with its estimate, how long it has run and what it is doing now. Exits 3 when the collector is not running.",
      },
      {
        usage: "runmarq wait",
        what: (
          <>
            Waits for the run in progress to end, showing its estimate on one line, then rings the terminal bell and exits with the
            outcome. <code>runmarq wait &amp;&amp; npm run deploy</code> deploys only after a clean finish. In Windows Terminal,
            ConEmu, Ghostty and WezTerm the run&apos;s progress also shows on the tab and the taskbar button.
          </>
        ),
        options: [
          ["--agent <claude|codex|cursor>", "Only wait for runs of this agent."],
          ["--repo <name>", "Only wait for runs in the repository with this folder name."],
          ["--timeout <seconds>", "Stop waiting after this long."],
          ["-q, --quiet", "No progress line, result or bell; only the exit code."],
        ],
      },
      {
        usage: "runmarq statusline",
        what: "One short line for a shell prompt, tmux or Claude Code’s status line, such as “runmarq ● 2 working · next: about 3–7 min”. Prints nothing when the collector is not running, so it never clutters a prompt.",
        options: [["--format <template>", "Your own line, with {working}, {waiting}, {stuck}, {next}, {agent} and {repo}."]],
      },
      {
        usage: "runmarq history",
        what: "Recent runs with their outcome, duration and first estimate.",
        options: [["--limit <n>", "How many runs to list (20 by default)."]],
      },
      {
        usage: "runmarq run -- <agent> [args…]",
        what: "Runs an agent under runmarq, passing its terminal, signals and exit code through unchanged, so runs are closed cleanly when the agent exits. Mainly for Codex.",
      },
    ],
  },
  {
    group: "Estimates and data",
    items: [
      {
        usage: "runmarq eval",
        what: (
          <>
            Replays the runs recorded on this computer in the order they ended, forecasting each with models fitted only on the runs
            before it, and reports how often the likely range, the full range and the headline held. See{" "}
            <Link href="/docs/eta-explained">how estimates work</Link>.
          </>
        ),
      },
      {
        usage: "runmarq export",
        what: "Run history as CSV: timings, outcomes and categories, never prompts or code.",
        options: [["--output <file>", "Write to this file instead of the terminal."]],
      },
      {
        usage: "runmarq forget",
        what: "Deletes run history after asking you to confirm.",
        options: [
          ["--before <YYYY-MM-DD>", "Delete only runs that started before this date."],
          ["--yes", "Skip the confirmation."],
        ],
      },
    ],
  },
  {
    group: "Setup",
    items: [
      {
        usage: "runmarq integrations setup <claude|codex|cursor>",
        what: "Shows the exact file and hook entries it will add, asks, writes them with a backup, and points them at a stable copy of runmarq.",
        options: [
          ["--dry-run", "Only show the change."],
          ["--project <dir>", "Configure one project instead of every project."],
          ["--yes", "Apply without asking."],
        ],
      },
      {
        usage: "runmarq integrations verify [agent]",
        what: "Checks each connected agent’s configuration.",
      },
      {
        usage: "runmarq integrations remove <agent>",
        what: "Removes only the entries runmarq added, leaving everything else as it was.",
      },
      {
        usage: "runmarq integrations list",
        what: "Every supported agent and the state of its integration.",
      },
      {
        usage: "runmarq collector start | stop | status",
        what: "Controls the background collector. The desktop app starts it for you; hooks keep working while it is stopped and their events are delivered later.",
        options: [["--foreground", "With start: run in this terminal instead of the background."]],
      },
      {
        usage: "runmarq doctor",
        what: (
          <>
            Checks the installation, collector, hook latency, model training and every integration, and says what to do next. Exits 1 when
            something needs attention. See <Link href="/docs/troubleshooting">troubleshooting</Link>.
          </>
        ),
      },
    ],
  },
];

export const WAIT_EXIT_CODES: Array<[string, string]> = [
  ["0", "The run finished."],
  ["1", "The run failed."],
  ["2", "The run was cancelled, or its outcome is unknown."],
  ["3", "No matching run is in progress."],
  ["124", "The timeout passed first."],
];
