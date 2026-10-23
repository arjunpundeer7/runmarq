use crate::event::ActivityCategory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Coarse stage of a run, derived from tool activity. A run moves between
/// phases until it ends; the phase model learns those transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Phase {
    /// From the prompt to the first tool call.
    Plan,
    Explore,
    Edit,
    Verify,
    Execute,
    Delegate,
    Compact,
}

impl Phase {
    pub const ALL: [Phase; 7] = [
        Phase::Plan,
        Phase::Explore,
        Phase::Edit,
        Phase::Verify,
        Phase::Execute,
        Phase::Delegate,
        Phase::Compact,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Phase> {
        Phase::ALL.get(i).copied()
    }

    pub fn of(activity: ActivityCategory) -> Phase {
        use ActivityCategory::*;
        match activity {
            ReadingFiles | SearchingRepository | Research => Phase::Explore,
            EditingCode => Phase::Edit,
            RunningTests | Building => Phase::Verify,
            RunningCommands | InstallingDependencies | Debugging | Unknown => Phase::Execute,
            Delegating => Phase::Delegate,
            CompactingContext => Phase::Compact,
            WaitingForApproval => Phase::Execute,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Plan => "plan",
            Phase::Explore => "explore",
            Phase::Edit => "edit",
            Phase::Verify => "verify",
            Phase::Execute => "execute",
            Phase::Delegate => "delegate",
            Phase::Compact => "compact",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Plan => "Planning",
            Phase::Explore => "Exploring",
            Phase::Edit => "Editing",
            Phase::Verify => "Testing and building",
            Phase::Execute => "Running commands",
            Phase::Delegate => "Delegating",
            Phase::Compact => "Compacting context",
        }
    }
}

/// Numbers observed during a run beyond hook events: from the agent's
/// transcript, from edit sizes, and from `git diff`. No text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct RunSignals {
    /// Model responses in the main conversation.
    pub turns: u32,
    pub subagent_turns: u32,
    #[ts(type = "number")]
    pub output_tokens: u64,
    #[ts(type = "number")]
    pub thinking_tokens: u64,
    /// Size of the most recent request, in tokens.
    #[ts(type = "number")]
    pub context_tokens: u64,
    #[ts(type = "number | null")]
    pub context_window: Option<u64>,
    pub compactions: u32,
    /// Lines added plus removed by edit tools, as reported in hook input.
    pub edit_lines: u32,
    /// Files changed in the working tree since the run started.
    pub diff_files: Option<u32>,
    /// Lines inserted plus deleted in the working tree since the run started.
    pub diff_lines: Option<u32>,
    #[ts(type = "string | null")]
    pub last_turn_at: Option<DateTime<Utc>>,
}

impl RunSignals {
    pub fn has_transcript_data(&self) -> bool {
        self.turns > 0 || self.output_tokens > 0
    }
}

/// Effort level as an ordinal for models: `None` when unknown.
pub fn effort_rank(effort: Option<&str>) -> Option<u8> {
    match effort?.to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some(0),
        "medium" => Some(1),
        "high" => Some(2),
        "xhigh" | "extra_high" | "very_high" => Some(3),
        "max" | "maximum" => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_cover_every_activity() {
        assert_eq!(Phase::of(ActivityCategory::RunningTests), Phase::Verify);
        assert_eq!(Phase::of(ActivityCategory::ReadingFiles), Phase::Explore);
        for (i, p) in Phase::ALL.iter().enumerate() {
            assert_eq!(p.index(), i);
            assert_eq!(Phase::from_index(i), Some(*p));
        }
    }

    #[test]
    fn effort_ranks() {
        assert_eq!(effort_rank(Some("High")), Some(2));
        assert_eq!(effort_rank(Some("xhigh")), Some(3));
        assert_eq!(effort_rank(Some("weird")), None);
        assert_eq!(effort_rank(None), None);
    }
}
