// A plain-text map of the site for coding agents and other readers that
// prefer text to markup, in the llms.txt format.
import { COMMANDS, commandId } from "@/content/commands";
import { DOCS } from "@/content/docs";
import { primaryCta, site } from "@/lib/site";

export const dynamic = "force-static";

const PAGES = [
  ["Features", "/features"],
  ["How it works", "/how-it-works"],
  ["Download", "/download"],
  ["Changelog", "/changelog"],
  ["Privacy", "/privacy"],
  ["Terms", "/terms"],
];

export function GET() {
  const url = (path: string) => new URL(path, site.url).toString();
  const text = [
    `# ${site.name}`,
    "",
    `> ${site.description}`,
    "",
    `Everything runs on the user's computer; estimates use no model calls and nothing is uploaded. To install: ${primaryCta.label} (${url(primaryCta.href)}).`,
    "",
    "## Docs",
    "",
    ...DOCS.map((d) => `- [${d.title}](${url(d.href)}): ${d.summary}`),
    "",
    "## Commands",
    "",
    ...COMMANDS.flatMap((g) => g.items).map((c) => `- [${c.usage}](${url(`/docs/command-line#${commandId(c.usage)}`)})`),
    "",
    "## Pages",
    "",
    ...PAGES.map(([title, path]) => `- [${title}](${url(path!)})`),
    "",
  ].join("\n");
  return new Response(text, { headers: { "content-type": "text/plain; charset=utf-8" } });
}
