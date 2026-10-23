import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";
import {
  ConnectRow,
  ModelPanel,
  NotificationToast,
  NotifyPanel,
  PhaseTimeline,
  RangeExplainer,
  RunActivity,
  TerminalPanel,
  TrayMenu,
  UsagePanel,
} from "@/components/fragments";
import { ContentPage, MoreLink, PageTitle } from "@/components/Prose";
import { ButtonLink } from "@/components/site";
import { COMMANDS, commandId, commandName } from "@/content/commands";
import { pageMetadata, primaryCta, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata(
  "/features",
  "Features",
  "Everything runmarq does: calibrated estimates learned from your own runs, live run activity, tray and notifications, history, insights, integrations and the runmarq command.",
);

interface Group {
  id: string;
  title: string;
  items: ReactNode[];
  frag: ReactNode;
  tone: string;
}

const code = (text: string) => <code className="font-mono text-[0.92em]">{text}</code>;

const GROUPS: Group[] = [
  {
    id: "estimates",
    title: "Estimates",
    items: [
      "A plain answer to whether to wait: almost done, a few minutes, time for something else, or a long task. It leads while the evidence is thin; the likely range leads once it is not.",
      "A likely range for the time left, and how long it could take when the tail runs long. Never a bare countdown.",
      "“Finishing up” when the work checks out and the agent stops starting tools, once your own runs show those signs mean the end.",
      "Low, medium or high confidence, from the evidence behind the estimate.",
      "A named reason whenever the estimate moves, such as repeated test failures.",
      "The estimate pauses while the agent waits for your approval.",
      "A run that outlasts similar runs gets a longer estimate instead of reaching zero.",
      "Messages you send mid-task and background-task notices continue the same run, so one task gets one estimate.",
      "Plan progress from Claude's task list or Codex's plan, when the agent keeps one.",
      "“Not enough information yet” rather than an invented number.",
    ],
    frag: <RangeExplainer />,
    tone: "bg-surface-2",
  },
  {
    id: "learning",
    title: "Learning",
    items: [
      "Four models on this computer. Once about 30 runs have finished, each is tested on your recent runs as if they were live and counts by how well it did.",
      "A history baseline from the first run, starting from deliberately wide built-in values.",
      "A phase simulation once ten runs with tool activity have finished.",
      "Similar earlier prompts, compared through salted word vectors.",
      "A gradient-boosted model after 100 finished runs, used only if it beats the others.",
      "Ranges calibrated to contain about 80% of outcomes once about 34 runs have finished.",
      "Retraining in the background as runs finish, without slowing the app.",
    ],
    frag: <ModelPanel />,
    tone: "bg-accent-muted/70",
  },
  {
    id: "signals",
    title: "Signals",
    items: [
      "Model responses, output and thinking tokens, context in use and compactions, from Claude Code and Codex transcripts.",
      "Files and lines changed in the working tree during the run, from git.",
      "Learned durations for the commands your agents run, such as your test suite.",
      "The effort level and model the agent reports.",
      "The current phase: planning, exploring, editing, testing and building, running commands, delegating or compacting context.",
      "The numbers only, never the text. Token counts, working-tree changes and prompt comparison can each be turned off in Settings → Estimation signals.",
    ],
    frag: <RunActivity />,
    tone: "bg-surface-2",
  },
  {
    id: "alerts",
    title: "Tray and alerts",
    items: [
      "A tray menu with every active run and its estimate.",
      "A compact panel for the corner of your screen, optionally kept on top.",
      "Notifications when a run finishes, fails, needs approval or stops reporting, with an optional sound.",
      "A warning when a run looks stuck: the same failure again and again, a command running far longer than usual, or a run long past its estimate with nothing happening.",
      "Quiet mode, and no finish or failure alerts for runs shorter than you choose.",
      "Clear run states: running, waiting for you, finished, failed, cancelled and status unknown.",
      "Starts when you log in, if you want it to.",
    ],
    frag: (
      <div className="flex w-full flex-col items-center gap-4 sm:flex-row sm:items-start sm:justify-center">
        <TrayMenu />
        <div className="flex w-full max-w-[340px] flex-col gap-4">
          <NotificationToast />
          <NotifyPanel />
        </div>
      </div>
    ),
    tone: "bg-accent-muted/70",
  },
  {
    id: "usage",
    title: "Usage",
    items: [
      "Output tokens, responses, runs and working time per agent, for today, the last 7 days and the current 5-hour window.",
      "When the current window started and when it resets.",
      "An optional budget per window that you set yourself, shown as progress.",
      "Counted from the transcripts on this computer. Not your plan's official meter: input and cached tokens are not counted, and runmarq does not know your plan's limits.",
    ],
    frag: <UsagePanel />,
    tone: "bg-surface-2",
  },
  {
    id: "terminal",
    title: "Terminal",
    items: [
      <>
        {code("runmarq wait")} blocks until the run finishes and exits 0 when it did, so {code("runmarq wait && npm run deploy")} runs only
        after a clean finish. Supporting terminals show its progress on the tab and taskbar.
      </>,
      <>
        {code("runmarq statusline")} prints one short line for a shell prompt, tmux or Claude Code&apos;s status line, and prints nothing
        when the collector is not running.
      </>,
    ],
    frag: <TerminalPanel />,
    tone: "bg-accent-muted/70",
  },
  {
    id: "history",
    title: "History and insights",
    items: [
      "History with search and filters by agent, status, repository and date.",
      "For each run: phases, events, active and waiting time, and every estimate change with its reason.",
      "A forecast chart that sets each estimate against the actual duration.",
      "Time spent waiting for approvals, and typical durations by agent and task type.",
      "Prediction quality on your machine: how often the range held, median error, and error by forecast age and agent.",
      "CSV export of your timing data.",
    ],
    frag: <PhaseTimeline />,
    tone: "bg-surface-2",
  },
  {
    id: "integrations",
    title: "Integrations",
    items: [
      "Claude Code, Codex and Cursor, each through its official hooks.",
      "Detects which agents are installed on this computer.",
      "Shows the exact file and entries before writing, and keeps a backup.",
      "Connects every project, or one project only.",
      "Marks an integration connected only once its first real event arrives.",
      "Removal takes out only the entries runmarq added.",
      <>{code("runmarq run -- codex")} closes Codex runs cleanly when the process exits.</>,
    ],
    frag: (
      <div className="flex w-full flex-col items-center gap-4 sm:flex-row sm:justify-center">
        <ConnectRow connected={false} />
        <ConnectRow connected />
      </div>
    ),
    tone: "bg-accent-muted/70",
  },
];

const PRIVATE = [
  "No account, no analytics and no network requests to compute an estimate.",
  "None of your agents' model usage is spent on estimates.",
  "Prompts are classified inside the hook process and discarded. Short titles are stored only if you turn them on.",
  "Source code, file contents, command arguments and output, and transcript text are never stored.",
  "Repository folder names can be hidden. History can be deleted at any time, or automatically after 30, 90 or 365 days.",
];

const LIMITS = [
  "It knows when an agent's turn ended, not whether your goal was met.",
  "It does not read terminal output or anything on your screen.",
  "Cursor's approval prompts are not visible to it, and agent sessions started without hooks cannot be observed.",
  "Estimates are wide until your own runs have been recorded.",
];

const sectionTitle = "font-serif text-[clamp(1.75rem,1.3rem+1.4vw,2.5rem)] font-medium leading-[1.08] tracking-[-0.025em]";

function Ticks({ items }: { items: ReactNode[] }) {
  return (
    <ul className="mt-6 space-y-3 text-[16px] text-fg-2">
      {items.map((item, i) => (
        <li key={i} className="grid grid-cols-[14px_1fr] gap-3">
          <span aria-hidden="true" className="mt-[0.6em] h-[2px] w-[14px] rounded-full bg-accent" />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

export default function Features() {
  return (
    <ContentPage>
      <PageTitle title="Features" />

      <nav aria-label="Features" className="-mt-4 mb-16 flex flex-wrap gap-2">
        {[...GROUPS, { id: "cli", title: "Command line" }, { id: "privacy", title: "Privacy" }].map((g) => (
          <a
            key={g.id}
            href={`#${g.id}`}
            className="rounded-full bg-surface px-4 py-2 text-[14.5px] text-fg-2 shadow-[0_0_0_1px_var(--rm-border)] transition-colors hover:text-fg hover:shadow-[0_0_0_1px_var(--rm-border-strong)]"
          >
            {g.title}
          </a>
        ))}
      </nav>

      <div className="space-y-24 md:space-y-32">
        {GROUPS.map((g, i) => (
          <section key={g.id} id={g.id} aria-labelledby={`${g.id}-title`} className="reveal grid scroll-mt-28 items-center gap-10 lg:grid-cols-12">
            <div className={i % 2 ? "lg:order-2 lg:col-span-5 lg:pl-4" : "lg:col-span-5"}>
              <h2 id={`${g.id}-title`} className={sectionTitle}>
                {g.title}
              </h2>
              <Ticks items={g.items} />
            </div>
            <div className={`flex justify-center rounded-[24px] px-4 py-10 sm:px-10 sm:py-14 lg:col-span-7 ${g.tone} ${i % 2 ? "lg:order-1" : ""}`}>
              {g.frag}
            </div>
          </section>
        ))}

        <section id="cli" aria-labelledby="cli-title" className="reveal scroll-mt-28">
          <h2 id="cli-title" className={sectionTitle}>
            Command line
          </h2>
          <ul className="mt-10 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {COMMANDS.flatMap((g) => g.items).map((c) => (
              <li key={c.usage}>
                <Link
                  href={`/docs/command-line#${commandId(c.usage)}`}
                  className="block rounded-[14px] bg-surface px-5 py-4 font-mono text-[14px] text-fg shadow-[0_0_0_1px_var(--rm-border)] transition-shadow hover:shadow-[0_0_0_1px_var(--rm-border-strong)]"
                >
                  {site.cli} {commandName(c.usage)}
                </Link>
              </li>
            ))}
          </ul>
        </section>

        <section id="privacy" aria-labelledby="privacy-title" className="reveal grid scroll-mt-28 gap-3 lg:grid-cols-2">
          <div className="rounded-[24px] bg-surface-2 p-6 sm:p-10">
            <h2 id="privacy-title" className={sectionTitle}>
              Privacy
            </h2>
            <Ticks items={PRIVATE} />
            <MoreLink href="/privacy">Privacy statement</MoreLink>
          </div>
          <div className="rounded-[24px] bg-surface p-6 shadow-[0_0_0_1px_var(--rm-border)] sm:p-10">
            <h2 className={sectionTitle}>What it does not do</h2>
            <Ticks items={LIMITS} />
            <MoreLink href="/docs/integrations/claude-code">Integration guides</MoreLink>
          </div>
        </section>

        <section aria-labelledby="start" className="reveal flex flex-col items-start gap-6 md:flex-row md:items-end md:justify-between">
          <h2 id="start" className="max-w-[18ch] text-heading leading-[1.05]">
            Try it on your next run.
          </h2>
          <div className="flex flex-wrap gap-3">
            <ButtonLink href={primaryCta.href}>{primaryCta.label}</ButtonLink>
            <ButtonLink href="/docs/getting-started" variant="ghost" arrow={false}>
              Getting started
            </ButtonLink>
          </div>
        </section>
      </div>
    </ContentPage>
  );
}
