// Small building blocks shared by every page: actions and the marks of the
// agents runmarq works with.
import { ArrowRight } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

export function ButtonLink({
  href,
  children,
  variant = "primary",
  size,
  arrow = variant === "primary",
  icon,
  className = "",
}: {
  href: string;
  children: ReactNode;
  variant?: "primary" | "ghost";
  size?: "sm";
  arrow?: boolean;
  icon?: ReactNode;
  className?: string;
}) {
  const cls = `btn btn-${variant} ${size === "sm" ? "btn-sm" : ""} ${className}`;
  const body = (
    <>
      {icon}
      {children}
      {arrow && <ArrowRight aria-hidden="true" size={size === "sm" ? 15 : 17} strokeWidth={2} className="btn-arrow" />}
    </>
  );
  return href.startsWith("/") ? (
    <Link href={href} className={cls}>
      {body}
    </Link>
  ) : (
    <a href={href} className={cls}>
      {body}
    </a>
  );
}

/** GitHub's mark, for links to the repository. */
export function GitHubMark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
    </svg>
  );
}

/** Simplified marks for "Works with". runmarq is not affiliated with these vendors. */
export function AgentMark({ agent, size = 22 }: { agent: "claude" | "codex" | "cursor"; size?: number }) {
  if (agent === "claude") {
    const rays = Array.from({ length: 12 }, (_, i) => i * 30);
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
        {rays.map((r, i) => (
          <line key={r} x1="12" y1="12" x2="12" y2={i % 2 ? 3.2 : 1.6} stroke="#d97757" strokeWidth="2.2" strokeLinecap="round" transform={`rotate(${r} 12 12)`} />
        ))}
      </svg>
    );
  }
  if (agent === "codex") {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
        {[0, 60, 120, 180, 240, 300].map((r) => (
          <rect key={r} x="9.1" y="2.6" width="5.8" height="12" rx="2.9" stroke="currentColor" strokeWidth="1.45" transform={`rotate(${r} 12 12)`} />
        ))}
      </svg>
    );
  }
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 1.8 21 7v10l-9 5.2L3 17V7z" fill="currentColor" />
      <path d="M12 12 21 7v10l-9 5.2z" fill="currentColor" opacity="0.55" />
      <path d="M3 7l9 5 9-5-9-5.2z" fill="var(--rm-bg)" opacity="0.92" />
      <path d="M12 12 21 7l-9 15.2z" fill="currentColor" opacity="0.9" />
    </svg>
  );
}

export function WorksWith({ className = "" }: { className?: string }) {
  const agents = [
    { id: "claude" as const, name: "Claude Code" },
    { id: "codex" as const, name: "Codex" },
    { id: "cursor" as const, name: "Cursor" },
  ];
  return (
    <ul aria-label="Works with" className={`flex flex-wrap items-center justify-center gap-x-7 gap-y-3 ${className}`}>
      {agents.map((a) => (
        <li key={a.id} className="flex items-center gap-2 text-[15px] font-medium text-fg">
          <AgentMark agent={a.id} />
          {a.name}
        </li>
      ))}
    </ul>
  );
}
