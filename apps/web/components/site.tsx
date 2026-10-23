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
