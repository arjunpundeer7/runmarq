import type { ReactNode } from "react";
import { DocsNav } from "@/components/DocsNav";
import { DocsSearch } from "@/components/DocsSearch";

export default function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <div className="mx-auto grid max-w-site gap-8 px-5 pt-10 sm:px-8 lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-16 lg:pt-16">
      <aside className="lg:sticky lg:top-24 lg:max-h-[calc(100dvh-7rem)] lg:self-start lg:overflow-y-auto">
        <DocsSearch />
        <div className="mt-6 hidden lg:block">
          <DocsNav />
        </div>
        <details className="mt-4 rounded-[14px] bg-surface-2 p-4 lg:hidden">
          <summary className="cursor-pointer text-[15px] font-medium">Browse documentation</summary>
          <div className="mt-4"><DocsNav /></div>
        </details>
      </aside>
      <article className="min-w-0 pb-28">{children}</article>
    </div>
  );
}
