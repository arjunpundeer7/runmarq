import { Play } from "lucide-react";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { ArchitectureDiagram } from "@/components/ArchitectureDiagram";
import { DemoRun } from "@/components/DemoRun";
import { ConnectRow, MiniRun, ModelPanel, NotificationToast, PhaseTimeline, RangeExplainer, RunActivity, TerminalPanel, TrayMenu } from "@/components/fragments";
import { IntegrationTabs } from "@/components/IntegrationTabs";
import { MoreLink } from "@/components/Prose";
import { ButtonLink, WorksWith } from "@/components/site";
import { pageMetadata, primaryCta, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata("/", null, site.description);

interface Story {
  id: string;
  title: string;
  body: ReactNode;
  frag: ReactNode;
  tone: string;
}

/** Each claim beside the piece of the app that backs it, alternating sides. */
const STORIES: Story[] = [
  {
    id: "expect",
    title: "Know what to expect.",
    body: (
      <>
        A likely range with a plain answer beside it: almost done, a few minutes, or time for something else. While the evidence is
        thin, the plain answer leads. The range tightens as the agent works, says why when it moves, and switches to &ldquo;Finishing
        up&rdquo; once your own runs show the work is wrapping up.
      </>
    ),
    frag: <RangeExplainer />,
    tone: "bg-surface-2",
  },
  {
    id: "checking",
    title: "Work without checking.",
    body: "The estimate lives in the tray. A quiet notification arrives when a run finishes, fails, needs your approval, or looks stuck.",
    frag: (
      <div className="relative flex w-full flex-col items-start gap-4 xl:min-h-[308px]">
        <TrayMenu />
        <NotificationToast className="self-end xl:absolute xl:bottom-0 xl:right-0" />
      </div>
    ),
    tone: "bg-accent-muted/70",
  },
  {
    id: "history",
    title: "See where the time went.",
    body: "Every run keeps its phases, waiting time and the estimates made along the way, so you can see what slows your agents down.",
    frag: <PhaseTimeline />,
    tone: "bg-surface-2",
  },
  {
    id: "learning",
    title: "Sharper with every run.",
    body: "Four models learn from the runs on this computer and, once enough have finished, count by how well each predicted your recent ones. None of it spends your agent's model usage.",
    frag: <ModelPanel />,
    tone: "bg-accent-muted/70",
  },
  {
    id: "signals",
    title: "The numbers, not the text.",
    body: "Token counts, context size and working-tree changes are read from the agent's transcript and from git on this computer. They sharpen the estimate; the text itself is never kept.",
    frag: <RunActivity />,
    tone: "bg-surface-2",
  },
  {
    id: "terminal",
    title: "In your terminal too.",
    body: (
      <>
        Chain your next step onto the agent finishing with <code className="font-mono text-[0.92em]">runmarq wait</code>, and keep every
        run in your prompt or Claude Code&apos;s status line.
      </>
    ),
    frag: <TerminalPanel />,
    tone: "bg-accent-muted/70",
  },
];

const STEPS = [
  { tone: "bg-surface-2", line: "Connect your coding agent.", frag: <ConnectRow connected={false} /> },
  {
    tone: "bg-accent-muted/70",
    line: "Keep working as usual.",
    frag: <MiniRun state="running" sub="Running tests · 03:14" text="A few minutes" detail="About 4–9 min" />,
  },
  { tone: "bg-surface-2", line: "Check the estimate when you need it.", frag: <TrayMenu /> },
];

const sectionTitle = "font-serif text-[clamp(1.75rem,1.3rem+1.4vw,2.5rem)] font-medium leading-[1.08] tracking-[-0.025em]";

export default function Home() {
  return (
    <>
      <section aria-labelledby="hero">
        <div className="hero mx-auto max-w-site px-5 text-center sm:px-8">
          <h1 id="hero" className="mx-auto max-w-[16ch] text-display leading-[1] tracking-[-0.035em] [font-weight:560] lg:max-w-none">
            <span className="mask-line">
              <span>Know when the</span>
            </span>{" "}
            <span className="mask-line">
              <span>work finishes.</span>
            </span>
          </h1>
          <div className="after-lines mt-[clamp(1.5rem,4svh,2.5rem)] flex flex-wrap items-center justify-center gap-3">
            <ButtonLink href={primaryCta.href}>{primaryCta.label}</ButtonLink>
            <ButtonLink href="/how-it-works" variant="ghost" icon={<Play aria-hidden="true" size={16} strokeWidth={2} />}>
              See how it works
            </ButtonLink>
          </div>
          <WorksWith className="after-lines mt-[clamp(1.25rem,3.2svh,2rem)]" />
          <div id="product" className="after-lines mx-auto mt-[clamp(1.5rem,3.6svh,2.5rem)] max-w-[640px] scroll-mt-24">
            <DemoRun autoplay />
          </div>
        </div>
      </section>

      <div className="mx-auto max-w-site space-y-20 px-5 py-24 sm:px-8 md:py-32 lg:space-y-28">
        {STORIES.map((s, i) => (
          <section key={s.id} aria-labelledby={s.id} className="reveal grid items-center gap-10 lg:grid-cols-12">
            <div className={i % 2 ? "lg:order-2 lg:col-span-5 lg:pl-6" : "lg:col-span-5"}>
              <h2 id={s.id} className={sectionTitle}>
                {s.title}
              </h2>
              <p className="mt-4 max-w-md text-[17px] text-fg-2">{s.body}</p>
            </div>
            <div className={`flex justify-center rounded-[24px] px-4 py-10 sm:px-10 sm:py-14 lg:col-span-7 ${s.tone} ${i % 2 ? "lg:order-1" : ""}`}>
              {s.frag}
            </div>
          </section>
        ))}
        <div className="flex justify-center">
          <ButtonLink href="/features" variant="ghost" arrow>
            Every feature
          </ButtonLink>
        </div>
      </div>

      <section aria-labelledby="how" className="py-24 md:py-32">
        <div className="mx-auto max-w-site px-5 sm:px-8">
          <div className="flex flex-wrap items-end justify-between gap-x-6">
            <h2 id="how" className="text-heading leading-[1.05]">
              Connect once. Keep working.
            </h2>
            <MoreLink href="/how-it-works">How it works</MoreLink>
          </div>
          <ol className="mt-12 grid gap-3 lg:grid-cols-3">
            {STEPS.map((f) => (
              <li key={f.line} className={`reveal flex min-h-[380px] flex-col justify-between rounded-[24px] p-5 sm:p-8 lg:min-h-[460px] ${f.tone}`}>
                <div className="flex justify-center pt-6 transition-transform duration-300 ease-out hover:-translate-y-1.5">{f.frag}</div>
                <p className="mt-10 max-w-xs font-serif text-[26px] leading-[1.15] tracking-[-0.02em]">{f.line}</p>
              </li>
            ))}
          </ol>
          <div className="reveal mt-16 rounded-[24px] bg-surface p-4 shadow-[0_0_0_1px_var(--rm-border)] sm:p-8">
            <ArchitectureDiagram />
          </div>
        </div>
      </section>

      <section aria-labelledby="integrations" className="mx-auto max-w-site px-5 py-24 sm:px-8 md:py-32">
        <h2 id="integrations" className="text-heading leading-[1.05]">
          Three agents. What each one reports.
        </h2>
        <div className="reveal mt-12">
          <IntegrationTabs />
        </div>
      </section>

      <section aria-labelledby="privacy" className="mx-auto max-w-site px-2 sm:px-3">
        <div className="rounded-[28px] bg-surface/55 px-3 py-20 shadow-[0_0_0_1px_var(--rm-border)] sm:px-5 md:py-28">
          <div className="mx-auto grid max-w-site gap-10 px-2 sm:px-5 lg:grid-cols-12 lg:gap-12">
            <div className="lg:col-span-5">
              <h2 id="privacy" className="text-heading leading-[1.05]">
                Your code stays yours.
              </h2>
              <MoreLink href="/privacy">Privacy statement</MoreLink>
            </div>
            <div className="reveal grid gap-3 sm:grid-cols-2 lg:col-span-7">
              <div className="rm-panel p-6 sm:p-7">
                <h3 className="text-[17px] font-semibold">Kept on this computer</h3>
                <ul className="mt-4 space-y-2.5 text-[15.5px] text-fg-2">
                  <li>When runs start, wait and end</li>
                  <li>Tool names and activity types</li>
                  <li>A task category from a local classifier</li>
                  <li>Token counts and change sizes, without the text</li>
                  <li>A salted repository identifier and its folder name, which you can turn off</li>
                  <li>Forecasts and how they compared with reality</li>
                </ul>
              </div>
              <div className="rm-panel p-6 sm:p-7">
                <h3 className="text-[17px] font-semibold">Never stored or sent</h3>
                <ul className="mt-4 space-y-2.5 text-[15.5px] text-fg-2">
                  <li>Prompt text, unless you turn on short titles</li>
                  <li>Source code and file contents</li>
                  <li>Command arguments and output</li>
                  <li>Transcript text and the names of changed files</li>
                  <li>Environment variables, API keys and secrets</li>
                  <li>Anything to a server: there is no account and no analytics</li>
                </ul>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section aria-labelledby="close" className="mt-12">
        <div className="mx-auto grid max-w-site items-center gap-12 px-5 py-24 sm:px-8 md:py-32 lg:grid-cols-12">
          <div className="lg:col-span-7">
            <h2 id="close" className="max-w-[16ch] text-[clamp(2.25rem,1rem+4.4vw,4.75rem)] leading-[1] tracking-[-0.035em]">
              Spend less time watching the terminal.
            </h2>
            <div className="mt-9 flex flex-wrap gap-3">
              <ButtonLink href={primaryCta.href}>{primaryCta.label}</ButtonLink>
              <ButtonLink href="/docs/getting-started" variant="ghost" arrow={false}>
                Read the docs
              </ButtonLink>
            </div>
          </div>
          <div className="flex justify-center lg:col-span-5 lg:justify-end">
            <MiniRun state="completed" sub="Run finished · estimated 4–12 min" text="8 min" />
          </div>
        </div>
      </section>
    </>
  );
}
