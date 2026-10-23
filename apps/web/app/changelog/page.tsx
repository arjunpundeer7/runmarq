import type { Metadata } from "next";
import { pageMetadata } from "@/lib/site";
import { ContentPage, PageTitle } from "@/components/Prose";
import { CHANGELOG } from "@/content/changelog";

export const metadata: Metadata = pageMetadata("/changelog", "Changelog", "What changed in each version of runmarq.");

export default function Changelog() {
  return (
    <ContentPage>
      <PageTitle title="Changelog" />
      <ol className="max-w-3xl space-y-14">
        {CHANGELOG.map((e) => (
          <li key={e.version}>
            <h2 id={`v${e.version}`} className="flex flex-wrap items-baseline gap-x-3 text-[32px] leading-tight">
              {e.version}
              <span className="text-[15px] font-normal text-muted">
                {e.status === "unreleased" ? "Unreleased" : <time dateTime={e.date}>{e.date}</time>}
              </span>
            </h2>
            <ul className="mt-4 space-y-2.5 text-[17px] text-fg-2">
              {e.items.map((i) => (
                <li key={i} className="pl-5 [text-indent:-1.25rem]">
                  <span aria-hidden="true" className="inline-block w-5 [text-indent:0] text-faint">
                    –
                  </span>
                  {i}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ol>
    </ContentPage>
  );
}
