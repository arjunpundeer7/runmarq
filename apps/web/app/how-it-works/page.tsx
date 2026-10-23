import type { Metadata } from "next";
import { pageMetadata } from "@/lib/site";
import { ArchitectureDiagram } from "@/components/ArchitectureDiagram";
import { CodeBlock } from "@/components/CodeBlock";
import { DemoRun } from "@/components/DemoRun";
import { ContentPage, MoreLink, PageTitle } from "@/components/Prose";

export const metadata: Metadata = pageMetadata(
  "/how-it-works",
  "How it works",
  "How runmarq follows a coding agent's run from its hooks to an estimate and a notification, with a demo run to step through.",
);

const STAGES = [
  {
    title: "The agent reports what it is doing",
    body: "Claude Code, Codex and Cursor each run runmarq's hook when something happens: a prompt is accepted, a tool starts or finishes, approval is needed, the turn ends. The hook prints only what the agent's hook protocol requires and always exits successfully.",
  },
  {
    title: "The hook keeps only timing and categories",
    body: "In the hook process, the prompt is classified (bug fix, feature, migration…) and dropped. Commands are inspected only to tell tests from builds and to recognise a repeated command by a salted hash. What leaves the hook is a small event: times, types, tool names, categories and salted hashes, never prompt text.",
  },
  {
    title: "The collector follows the run",
    body: "A background process on this computer turns events into runs with an explicit state: running, waiting for you, finished, failed, cancelled or unknown. It separates active time from time spent waiting for you, and never marks an unclear ending as finished.",
  },
  {
    title: "The estimator predicts what is left",
    body: "Each meaningful event, and every fifteen seconds of quiet work, produces a new estimate: an answer to whether to wait, and a likely range for the time left. Several models trained on this machine's runs contribute, weighted, once enough runs have finished, by how well each has predicted your recent ones. When the work checks out, the agent stops starting tools and your earlier runs show what that means, it says “Finishing up”. Nothing is sent anywhere to compute it.",
  },
  {
    title: "You find out without looking",
    body: "The tray shows every active run's estimate. A notification arrives when a run finishes, fails, needs your approval or looks stuck, and each run's timeline stays in history. In a terminal, runmarq wait returns when the run ends and runmarq statusline keeps it in your prompt.",
  },
];

export default function HowItWorks() {
  return (
    <ContentPage>
      <PageTitle title="How it works" />

      <section aria-label="Demo run" className="rounded-[28px] bg-surface-2 p-3 sm:p-8 md:p-12">
        <div className="mx-auto max-w-[720px]">
          <DemoRun />
        </div>
      </section>

      <section aria-labelledby="path" className="mt-28">
        <h2 id="path" className="text-heading leading-[1.05]">
          From a hook to a notification
        </h2>
        <ol className="mt-12 grid gap-3 md:grid-cols-2">
          {STAGES.map((s) => (
            <li key={s.title} className="reveal rounded-[20px] bg-surface p-6 shadow-[0_0_0_1px_var(--rm-border)] sm:p-8">
              <h3 className="font-serif text-[24px] leading-tight tracking-[-0.02em]">{s.title}</h3>
              <p className="mt-2 text-[17px] text-fg-2">{s.body}</p>
            </li>
          ))}
        </ol>
        <div className="reveal mt-10 rounded-[24px] bg-surface p-4 shadow-[0_0_0_1px_var(--rm-border)] sm:p-8">
          <ArchitectureDiagram />
        </div>
      </section>

      <section aria-labelledby="setup" className="mt-28 grid gap-10 md:grid-cols-12">
        <div className="md:col-span-5">
          <h2 id="setup" className="text-heading leading-[1.05]">
            Setup
          </h2>
          <p className="mt-4 text-[17px] text-fg-2">
            Connecting an agent means adding a few hook entries to its configuration. You see the file and the exact entries before
            anything is written, a backup is kept, and removal takes out only what was added. The desktop app does the same on first launch.
          </p>
          <MoreLink href="/docs/integrations/claude-code">Integration guides</MoreLink>
        </div>
        <div className="md:col-span-7">
          <CodeBlock>{`runmarq integrations setup claude --dry-run
runmarq integrations setup claude
runmarq integrations remove claude`}</CodeBlock>
        </div>
      </section>

      <section aria-labelledby="finished" className="mt-28 max-w-3xl">
        <h2 id="finished" className="text-heading leading-[1.05]">
          What “finished” means
        </h2>
        <p className="mt-4 text-[17px] text-fg-2">
          runmarq can tell when an agent&apos;s turn has ended. It cannot tell whether your broader goal was met. So the app says “Run
          finished” when the agent reports the end of its turn, “Failed” when it reports an error, “Cancelled” when you interrupted it or
          closed it, and “Status unknown” when it cannot tell.
        </p>
        <MoreLink href="/docs/eta-explained">How estimates work</MoreLink>
      </section>
    </ContentPage>
  );
}
