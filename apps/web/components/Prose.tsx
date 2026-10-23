import { ArrowRight } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

export function PageTitle({ title }: { title: string }) {
  return <h1 className="mb-12 max-w-3xl text-[clamp(2.375rem,1.3rem+3.6vw,4.5rem)] leading-[1] tracking-[-0.035em]">{title}</h1>;
}

export function Prose({ children }: { children: ReactNode }) {
  return <div className="prose-rm">{children}</div>;
}

export function Note({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "warn" }) {
  return (
    <div className={`my-6 rounded-[14px] px-5 py-4 text-[16px] ${tone === "warn" ? "bg-warn-muted text-fg" : "bg-surface-2 text-fg-2"}`}>
      {children}
    </div>
  );
}

/** Standard page shell for non-docs content pages. */
export function ContentPage({ children }: { children: ReactNode }) {
  return <div className="mx-auto max-w-site px-5 pb-28 pt-14 sm:px-8 md:pt-20">{children}</div>;
}

/** A plain link that reads as the next step, not as a sentence. */
export function MoreLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link href={href} className="group mt-6 inline-flex items-center gap-1.5 text-[16px] font-medium text-fg">
      <span className="underline decoration-line-strong underline-offset-[6px] transition-colors group-hover:decoration-accent">{children}</span>
      <ArrowRight aria-hidden="true" size={16} strokeWidth={2} className="transition-transform group-hover:translate-x-0.5" />
    </Link>
  );
}
