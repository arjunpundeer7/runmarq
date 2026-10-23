import { Wordmark } from "@runmarq/ui";
import Link from "next/link";
import { primaryCta, site } from "@/lib/site";
import { MobileNav } from "./MobileNav";
import { ButtonLink } from "./site";

export const NAV_LINKS = [
  { href: "/features", label: "Features" },
  { href: "/how-it-works", label: "How it works" },
  { href: "/docs", label: "Documentation" },
  ...(site.githubUrl ? [{ href: site.githubUrl, label: "GitHub" }] : []),
  { href: "/download", label: "Download" },
];

export function Header() {
  return (
    <header className="site-header sticky top-0 z-30">
      <div className="mx-auto grid h-[72px] max-w-site grid-cols-[1fr_auto] items-center px-5 sm:px-8 lg:grid-cols-[1fr_auto_1fr]">
        <Link href="/" aria-label={`${site.name} home`} className="justify-self-start rounded-md">
          <Wordmark name={site.name} size={26} className="gap-2.5 [&>span]:text-[19px]" />
        </Link>
        <nav aria-label="Main" className="hidden items-center gap-8 lg:flex xl:gap-10">
          {NAV_LINKS.map((l) => (
            <Link key={l.href} href={l.href} className="text-[15px] text-fg-2 transition-colors hover:text-fg">
              {l.label}
            </Link>
          ))}
        </nav>
        <div className="hidden justify-self-end lg:block">
          <ButtonLink href={primaryCta.href} size="sm">
            {primaryCta.label}
          </ButtonLink>
        </div>
        <MobileNav links={NAV_LINKS} cta={primaryCta} />
      </div>
    </header>
  );
}
