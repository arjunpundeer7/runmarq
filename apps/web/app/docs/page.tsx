import type { Metadata } from "next";
import { pageMetadata } from "@/lib/site";
import Link from "next/link";
import { PageTitle } from "@/components/Prose";
import { DOCS } from "@/content/docs";

export const metadata: Metadata = pageMetadata(
  "/docs",
  "Documentation",
  "Guides and reference for installing runmarq, connecting Claude Code, Codex and Cursor, and reading its estimates.",
);

export default function DocsIndex() {
  return (
    <>
      <PageTitle title="Documentation" />
      <ul className="grid gap-3 sm:grid-cols-2">
        {DOCS.map((d) => (
          <li key={d.href}>
            <Link
              href={d.href}
              className="block h-full rounded-[18px] bg-surface p-6 font-serif text-[24px] leading-tight tracking-[-0.02em] shadow-[0_0_0_1px_var(--rm-border)] transition-[box-shadow,transform,color] duration-200 hover:-translate-y-0.5 hover:text-accent-text hover:shadow-[0_0_0_1px_var(--rm-border-strong),var(--rm-shadow)]"
            >
              {d.title}
            </Link>
          </li>
        ))}
      </ul>
    </>
  );
}
