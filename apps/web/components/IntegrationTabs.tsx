"use client";

import type { AgentCapabilities } from "@runmarq/contracts";
import { CAPABILITY_LABELS, INTEGRATIONS } from "@runmarq/contracts";
import { cn } from "@runmarq/ui";
import { Check, Minus } from "lucide-react";
import { useState } from "react";
import { MoreLink } from "./Prose";

/** Capabilities per agent, straight from the generated manifest. */
export function IntegrationTabs() {
  const [active, setActive] = useState(INTEGRATIONS[0]!.id);
  const entry = INTEGRATIONS.find((i) => i.id === active)!;
  const keys = Object.keys(CAPABILITY_LABELS) as Array<keyof AgentCapabilities>;

  return (
    <div className="grid gap-6 lg:grid-cols-[180px_minmax(0,1fr)] lg:gap-10">
      <div role="tablist" aria-label="Agents" className="flex flex-wrap gap-1.5 lg:flex-col">
        {INTEGRATIONS.map((i) => (
          <button
            key={i.id}
            role="tab"
            type="button"
            id={`tab-${i.id}`}
            aria-selected={i.id === active}
            aria-controls={`panel-${i.id}`}
            onClick={() => setActive(i.id)}
            className={cn(
              "min-w-[8rem] flex-1 rounded-[12px] px-4 py-3 text-left text-[16px] font-medium transition-colors lg:flex-none",
              i.id === active ? "bg-fg text-bg" : "bg-surface-2 text-fg hover:bg-line",
            )}
          >
            {i.displayName}
          </button>
        ))}
      </div>

      <div role="tabpanel" id={`panel-${entry.id}`} aria-labelledby={`tab-${entry.id}`}>
        <ul className="grid gap-x-8 sm:grid-cols-2">
          {keys.map((k) => {
            const on = entry.capabilities[k];
            return (
              <li key={k} className="flex items-center gap-3 border-b border-line/70 py-3">
                <span role="img" className={on ? "text-ok" : "text-faint"} aria-label={on ? "Supported" : "Not available"}>
                  {on ? <Check size={16} /> : <Minus size={16} />}
                </span>
                <span className={cn("text-[16px]", on ? "font-medium" : "text-muted")}>{CAPABILITY_LABELS[k]}</span>
              </li>
            );
          })}
        </ul>
        {entry.limitations.length > 0 && (
          <ul className="mt-6 max-w-[70ch] space-y-1.5 rounded-[12px] bg-warn-muted/60 p-5 text-[15px] text-fg-2">
            {entry.limitations.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
        )}
        <MoreLink href={entry.docsPath}>{entry.displayName} setup</MoreLink>
      </div>
    </div>
  );
}
