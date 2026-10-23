import type { Metadata } from "next";
import { ContentPage, Note, PageTitle, Prose } from "@/components/Prose";
import { pageMetadata, released, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata("/terms", "Terms", "Plain-language terms for using runmarq and this website.");

export default function Terms() {
  return (
    <ContentPage>
      <PageTitle title="Terms" />
      <Prose>
        {!released && (
          <Note tone="warn">
            {site.name} is pre-release software with no public build. These terms are a plain-language placeholder until formal terms are
            published with the first release; they are not legal advice and have not been reviewed by a lawyer.
          </Note>
        )}
        <h2>Use of the software</h2>
        <p>
          {site.name} is provided as is, without warranties of any kind. Estimates are statistical predictions and can be wrong; do not
          rely on them for anything where a wrong estimate would cause harm.
        </p>
        <h2>Changes to agent configuration</h2>
        <p>
          With your approval, {site.name} adds hook entries to the configuration files of the coding agents you connect. It keeps a backup
          before every change and removes only its own entries when you disconnect an agent. You are responsible for reviewing changes to
          configuration you share with others.
        </p>
        <h2>Third-party products</h2>
        <p>
          Claude Code, Codex and Cursor are products of their respective companies. {site.name} is not affiliated with or endorsed by
          Anthropic, OpenAI or Anysphere, and works only through the interfaces those products document.
        </p>
        <h2>This website</h2>
        <p>Content on this site describes the software as built. Where something is planned rather than available, the page says so.</p>
        <p>
          Last updated <time dateTime="2026-10-21">21 October 2026</time>.
        </p>
      </Prose>
    </ContentPage>
  );
}
