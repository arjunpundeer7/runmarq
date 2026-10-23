import type { Metadata } from "next";
import { ContentPage, MoreLink, PageTitle, Prose } from "@/components/Prose";
import { pageMetadata, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata("/contact", "Contact", "How to reach the people behind runmarq and report a problem.");

export default function Contact() {
  return (
    <ContentPage>
      <PageTitle title="Contact" />
      <Prose>
        {site.contactEmail ? (
          <p className="text-lead">
            Email <a href={`mailto:${site.contactEmail}`}>{site.contactEmail}</a>. Include the output of <code>runmarq doctor --json</code> if
            you are reporting a problem; it contains no prompts or code.
          </p>
        ) : site.githubUrl ? (
          <p className="text-lead">
            Open an issue on <a href={site.githubUrl}>GitHub</a>. Include the output of <code>runmarq doctor --json</code> if you are
            reporting a problem; it contains no prompts or code.
          </p>
        ) : (
          <p className="text-lead">There is no public contact address before the first release.</p>
        )}
      </Prose>
      <MoreLink href="/docs/troubleshooting">Troubleshooting</MoreLink>
    </ContentPage>
  );
}
