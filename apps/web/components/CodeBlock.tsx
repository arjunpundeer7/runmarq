"use client";

import { Check, Copy } from "lucide-react";
import { useState } from "react";

/** A copyable command or code sample. */
export function CodeBlock({ children, label }: { children: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(children);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="group relative my-5 max-w-full rounded-[14px] bg-code text-code-fg ring-1 ring-code-edge">
      {label && <p className="px-4 pt-3 text-[13px] text-code-fg/60">{label}</p>}
      {/* Focusable so a keyboard can scroll a long line into view. */}
      <pre tabIndex={0} className="overflow-x-auto px-4 py-3.5 pr-14 font-mono text-[14px] leading-relaxed focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-code-fg/60">
        <code>{children}</code>
      </pre>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy to clipboard"}
        className="absolute right-2.5 top-2.5 inline-flex size-8 items-center justify-center rounded-lg text-code-fg/70 transition-colors hover:bg-code-fg/10 hover:text-code-fg"
      >
        {copied ? <Check size={15} /> : <Copy size={15} />}
      </button>
    </div>
  );
}
