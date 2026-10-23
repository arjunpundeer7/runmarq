import { Wordmark } from "@runmarq/ui";
import Link from "next/link";
import { primaryCta, site } from "@/lib/site";
import { ButtonLink } from "./site";

const COLUMNS = [
  {
    title: "Product",
    links: [
      { href: "/#product", label: "Overview" },
      { href: "/features", label: "Features" },
      { href: "/how-it-works", label: "How it works" },
      { href: "/download", label: "Downloads" },
      { href: "/changelog", label: "Changelog" },
    ],
  },
  {
    title: "Documentation",
    links: [
      { href: "/docs/getting-started", label: "Getting started" },
      { href: "/docs/eta-explained", label: "How estimates work" },
      { href: "/docs/command-line", label: "Command line" },
      { href: "/docs/troubleshooting", label: "Troubleshooting" },
    ],
  },
  {
    title: "Company",
    links: [
      { href: "/privacy", label: "Privacy" },
      { href: "/terms", label: "Terms" },
      { href: "/contact", label: "Contact" },
      ...(site.githubUrl ? [{ href: site.githubUrl, label: "GitHub" }] : []),
    ],
  },
];

/** The close of every page: the links, then the name set large in the serif. */
export function Footer() {
  return (
    <footer className="border-t border-line/80">
      <div className="mx-auto grid max-w-site gap-12 px-5 pb-10 pt-16 sm:px-8 md:pt-20 lg:grid-cols-12">
        <div className="flex flex-col items-start gap-7 lg:col-span-5">
          <Wordmark name={site.name} size={24} className="gap-2.5 [&>span]:text-[18px]" />
          <ButtonLink href={primaryCta.href} size="sm">
            {primaryCta.label}
          </ButtonLink>
        </div>
        <div className="grid grid-cols-2 gap-8 sm:grid-cols-3 lg:col-span-6 lg:col-start-7">
          {COLUMNS.map((c) => (
            <nav key={c.title} aria-label={`${c.title} links`}>
              <ul className="space-y-2.5">
                {c.links.map((l) => (
                  <li key={l.href}>
                    <Link href={l.href} className="text-[15.5px] text-fg-2 transition-colors hover:text-accent-text">
                      {l.label}
                    </Link>
                  </li>
                ))}
              </ul>
            </nav>
          ))}
        </div>
      </div>
      <div className="@container mx-auto max-w-site overflow-hidden px-5 sm:px-8">
        <span
          aria-hidden="true"
          className="block select-none whitespace-nowrap font-serif text-[clamp(3.5rem,21cqi,17rem)] font-medium pb-[0.04em] pt-[0.12em] leading-[0.86] tracking-[-0.05em] text-fg"
        >
          {site.name}
          <span className="text-accent">.</span>
        </span>
      </div>
      <p className="mx-auto max-w-site px-5 pb-8 pt-8 text-[13.5px] text-muted sm:px-8">
        {site.name} is not affiliated with Anthropic, OpenAI or Anysphere.
      </p>
    </footer>
  );
}
