import { useId } from "react";
import { cn } from "./cn";
import { LOGO_RIBBON } from "./logo-ribbon";

const VIEW = { x: 116, y: 142, w: 1087, h: 899 };

/**
 * The runmarq symbol: a ribbon R with speed lines and a clock hand in its
 * counter. The ribbon keeps its own colours; the hand follows currentColor,
 * warming toward the accent at its bend, so the mark reads on light and dark.
 * `size` is the height in pixels.
 */
export function LogoMark({ size = 20, className }: { size?: number; className?: string }) {
  const gradient = `rm-hand-${useId().replace(/[^\w-]/g, "")}`;
  return (
    <svg
      width={Math.round((size * VIEW.w) / VIEW.h)}
      height={size}
      viewBox={`${VIEW.x} ${VIEW.y} ${VIEW.w} ${VIEW.h}`}
      fill="none"
      aria-hidden="true"
      className={cn("shrink-0", className)}
    >
      <defs>
        <radialGradient id={gradient} gradientUnits="userSpaceOnUse" cx="757" cy="527" r="100">
          <stop offset="0" style={{ stopColor: "color-mix(in srgb, var(--rm-accent) 70%, currentColor)" }} />
          <stop offset="1" stopColor="currentColor" />
        </radialGradient>
      </defs>
      <image href={LOGO_RIBBON} x={VIEW.x} y={VIEW.y} width={VIEW.w} height={VIEW.h} />
      <path
        d="M821 392L757 527L891 529"
        stroke={`url(#${gradient})`}
        strokeWidth="62"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function Wordmark({ name, size = 20, className }: { name: string; size?: number; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2 text-fg", className)}>
      <LogoMark size={size} />
      <span className="text-[15px] font-semibold tracking-[-0.01em]">{name}</span>
    </span>
  );
}
