"use client";

import { Search } from "lucide-react";
import Link from "next/link";
import { useMemo, useState } from "react";
import { DOCS } from "@/content/docs";

/** Small client-side search over the documentation index. */
export function DocsSearch() {
  const [q, setQ] = useState("");
  const results = useMemo(() => {
    const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
    if (!terms.length) return [];
    return DOCS.filter((d) => {
      const hay = `${d.title} ${d.summary} ${d.keywords}`.toLowerCase();
      return terms.every((t) => hay.includes(t));
    });
  }, [q]);

  return (
    <div className="relative">
      <label className="relative block">
        <span className="sr-only">Search documentation</span>
        <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />
        <input
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search docs"
          className="h-10 w-full rounded-full bg-surface-2 pl-9 pr-3 text-[15px] outline-none placeholder:text-muted focus-visible:ring-2 focus-visible:ring-accent"
        />
      </label>
      {q.trim() && (
        <ul className="mt-2 space-y-1" aria-live="polite">
          {results.length === 0 && <li className="px-3 py-2 text-[14px] text-muted">No pages match.</li>}
          {results.map((r) => (
            <li key={r.href}>
              <Link href={r.href} onClick={() => setQ("")} className="block rounded-lg px-3 py-2 text-[15px] font-medium hover:bg-surface-2">
                {r.title}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
