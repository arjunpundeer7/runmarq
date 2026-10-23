// Documentation index: navigation order, and the text used by search, page
// descriptions and llms.txt.
import type { Metadata } from "next";
import { pageMetadata } from "@/lib/site";
export interface DocPage {
  href: string;
  title: string;
  summary: string;
  keywords: string;
}

export const DOCS: DocPage[] = [
  {
    href: "/docs/getting-started",
    title: "Getting started",
    summary: "Install runmarq, connect an agent, and see your first estimate.",
    keywords: "install setup onboarding collector start first run cli desktop build from source",
  },
  {
    href: "/docs/integrations/claude-code",
    title: "Claude Code",
    summary: "Hooks in settings.json, what is captured, and how interrupts are handled.",
    keywords: "claude hooks settings.json userpromptsubmit pretooluse stop notification permission",
  },
  {
    href: "/docs/integrations/codex",
    title: "Codex",
    summary: "Lifecycle hooks in hooks.json, trusting them with /hooks, and the runmarq run wrapper.",
    keywords: "codex hooks.json trust /hooks exec json wrapper run turn_id",
  },
  {
    href: "/docs/integrations/cursor",
    title: "Cursor",
    summary: "Cursor agent hooks, the observer-only event set, and Windows notes.",
    keywords: "cursor hooks.json beforesubmitprompt posttooluse stop windows shell",
  },
  {
    href: "/docs/eta-explained",
    title: "How estimates work",
    summary: "The four local models, how they are weighted, conformal calibration, and what the range means.",
    keywords: "eta estimate outlook finishing up eval backtest pace p10 p50 p90 confidence range lognormal history phase simulation similar prompts gradient boosting conformal calibration coverage accuracy cold start transcripts tokens",
  },
  {
    href: "/docs/command-line",
    title: "Command line",
    summary: "Every runmarq command, including wait, statusline and eval, with options and exit codes.",
    keywords: "cli command line terminal wait statusline status line tmux prompt eval backtest history export forget run doctor exit codes json",
  },
  {
    href: "/docs/troubleshooting",
    title: "Troubleshooting",
    summary: "runmarq doctor, common problems and where the logs are.",
    keywords: "doctor logs spool collector not running permission connected waiting first event problem error tokens transcript git working tree model not in use",
  },
];

/** Metadata for a documentation page, from its entry above. */
export function docMetadata(href: string): Metadata {
  const doc = DOCS.find((d) => d.href === href);
  if (!doc) throw new Error(`no documentation entry for ${href}`);
  return pageMetadata(href, doc.title, doc.summary);
}
