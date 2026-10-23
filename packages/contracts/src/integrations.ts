// Typed view of integrations.json, which is generated from the Rust adapters
// (`runmarq integrations manifest`) and checked by a Rust test, so the website
// can only claim capabilities the code actually has.
import type { AgentCapabilities, AgentSource, SupportLevel } from "./generated";
import manifest from "../integrations.json";

export interface IntegrationManifestEntry {
  id: AgentSource;
  displayName: string;
  support: SupportLevel;
  capabilities: AgentCapabilities;
  limitations: string[];
  mechanism: string;
  docsPath: string;
}

export const INTEGRATIONS = manifest as IntegrationManifestEntry[];

export const SUPPORT_LABELS: Record<SupportLevel, string> = {
  available: "Available",
  limited: "Limited",
  in_development: "In development",
};

export const CAPABILITY_LABELS: Record<keyof AgentCapabilities, string> = {
  promptStart: "Prompt start",
  turnCompletion: "Turn completion",
  toolStarted: "Tool start",
  toolCompleted: "Tool completion",
  planUpdates: "Plan progress",
  approvalEvents: "Approval requests",
  errorEvents: "Errors",
  subagents: "Subagents",
  modelMetadata: "Model metadata",
  transcriptUsage: "Token counts",
  interruptEvents: "Interrupts reported directly",
};
