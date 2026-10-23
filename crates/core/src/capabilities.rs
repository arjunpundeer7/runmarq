//! Machine-readable capability matrix. The UI enables a feature
//! only when the adapter reports the capability.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentCapabilities {
    pub prompt_start: bool,
    pub turn_completion: bool,
    pub tool_started: bool,
    pub tool_completed: bool,
    pub plan_updates: bool,
    pub approval_events: bool,
    pub error_events: bool,
    pub subagents: bool,
    pub model_metadata: bool,
    /// Token counts and effort can be read from the agent's session transcript.
    pub transcript_usage: bool,
    /// The vendor reports user interrupts directly (otherwise they are inferred).
    pub interrupt_events: bool,
}

/// Public support level, shown on the website and in the desktop app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SupportLevel {
    Available,
    Limited,
    InDevelopment,
}
