"use client";

import { cn } from "@runmarq/ui";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { DOCS } from "@/content/docs";

/** Start, integrations and reference, separated by space rather than labels. */
const GROUPS = [
  ["/docs", "/docs/getting-started"],
  ["/docs/integrations/claude-code", "/docs/integrations/codex", "/docs/integrations/cursor"],
  ["/docs/eta-explained", "/docs/command-line", "/docs/troubleshooting"],
];

export function DocsNav() {
  const path = usePathname();
  const title = (href: string) => (href === "/docs" ? "Overview" : (DOCS.find((d) => d.href === href)?.title ?? href));
  return (
    <nav aria-label="Documentation" className="space-y-5">
      {GROUPS.map((items) => (
        <ul key={items[0]} className="space-y-0.5">
          {items.map((href) => (
            <li key={href}>
              <Link
                href={href}
                aria-current={path === href ? "page" : undefined}
                className={cn(
                  "block rounded-lg px-3 py-1.5 text-[15px] transition-colors",
                  path === href ? "bg-accent-muted/70 font-medium text-fg" : "text-fg-2 hover:bg-surface-2 hover:text-fg",
                )}
              >
                {title(href)}
              </Link>
            </li>
          ))}
        </ul>
      ))}
    </nav>
  );
}
