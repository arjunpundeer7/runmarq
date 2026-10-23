import type { AgentCapabilities } from "@runmarq/contracts";
import { CAPABILITY_LABELS, INTEGRATIONS, SUPPORT_LABELS } from "@runmarq/contracts";
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { PageTitle, Prose } from "@/components/Prose";
import { DOCS } from "@/content/docs";
import { AGENT_DOCS } from "@/content/integrations";
import { pageMetadata } from "@/lib/site";

export function generateStaticParams() {
  return AGENT_DOCS.map((d) => ({ agent: d.slug }));
}

export const dynamicParams = false;

export async function generateMetadata({ params }: { params: Promise<{ agent: string }> }): Promise<Metadata> {
  const { agent } = await params;
  const doc = AGENT_DOCS.find((d) => d.slug === agent);
  const entry = INTEGRATIONS.find((i) => i.id === doc?.id);
  if (!doc || !entry) return { title: "Integration" };
  const path = `/docs/integrations/${doc.slug}`;
  const summary = DOCS.find((d) => d.href === path)?.summary ?? `How runmarq connects to ${entry.displayName}.`;
  return pageMetadata(path, `${entry.displayName} integration`, summary);
}

export default async function AgentPage({ params }: { params: Promise<{ agent: string }> }) {
  const { agent } = await params;
  const doc = AGENT_DOCS.find((d) => d.slug === agent);
  const entry = INTEGRATIONS.find((i) => i.id === doc?.id);
  if (!doc || !entry) notFound();
  const keys = Object.keys(CAPABILITY_LABELS) as Array<keyof AgentCapabilities>;

  return (
    <>
      <PageTitle title={entry.displayName} />
      <Prose>
        {doc.body}
        <h2>Compatibility</h2>
        <table>
          <tbody>
            <tr>
              <td>Connection</td>
              <td>{entry.mechanism}</td>
            </tr>
            <tr>
              <td>Support</td>
              <td>{SUPPORT_LABELS[entry.support]}</td>
            </tr>
            <tr>
              <td>Checked against</td>
              <td>{doc.testedWith}</td>
            </tr>
          </tbody>
        </table>
        <h2>Capabilities</h2>
        <table>
          <thead>
            <tr>
              <th>Signal</th>
              <th>Available</th>
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => (
              <tr key={k}>
                <td>{CAPABILITY_LABELS[k]}</td>
                <td>{entry.capabilities[k] ? "Yes" : "No"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {entry.limitations.length > 0 && (
          <>
            <h2>Known limitations</h2>
            <ul>
              {entry.limitations.map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </>
        )}
      </Prose>
    </>
  );
}
