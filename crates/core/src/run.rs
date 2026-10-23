//! Explicit run state machine.
//!
//! A run is one user-triggered agent turn. Terminal states are final: late or
//! duplicated events can never resurrect or re-finish a run. Ambiguity is
//! represented as `Unknown`, never silently converted into `Completed`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunState {
    Created,
    Estimating,
    Running,
    WaitingForUser,
    WaitingForExternalResource,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("illegal run transition {from:?} -> {to:?}")]
pub struct TransitionError {
    pub from: RunState,
    pub to: RunState,
}

impl RunState {
    pub const ALL: [RunState; 10] = [
        RunState::Created,
        RunState::Estimating,
        RunState::Running,
        RunState::WaitingForUser,
        RunState::WaitingForExternalResource,
        RunState::Blocked,
        RunState::Completed,
        RunState::Failed,
        RunState::Cancelled,
        RunState::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Created => "created",
            RunState::Estimating => "estimating",
            RunState::Running => "running",
            RunState::WaitingForUser => "waiting_for_user",
            RunState::WaitingForExternalResource => "waiting_for_external_resource",
            RunState::Blocked => "blocked",
            RunState::Completed => "completed",
            RunState::Failed => "failed",
            RunState::Cancelled => "cancelled",
            RunState::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        RunState::ALL.into_iter().find(|st| st.as_str() == s)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunState::Completed | RunState::Failed | RunState::Cancelled
        )
    }

    /// Nonterminal states. `Unknown` counts: it is unresolved, not finished.
    pub fn is_open(self) -> bool {
        !self.is_terminal()
    }

    /// States in which the agent is believed to be doing work on its own.
    pub fn is_working(self) -> bool {
        matches!(
            self,
            RunState::Created | RunState::Estimating | RunState::Running | RunState::Blocked
        )
    }

    pub fn is_waiting(self) -> bool {
        matches!(
            self,
            RunState::WaitingForUser | RunState::WaitingForExternalResource
        )
    }

    /// Legal transitions. Self-transitions are handled by callers as no-ops.
    pub fn can_transition_to(self, to: RunState) -> bool {
        use RunState::*;
        if self == to {
            return true;
        }
        match self {
            Completed | Failed | Cancelled => false,
            Created => to != Created,
            Estimating => !matches!(to, Created),
            Running => !matches!(to, Created | Estimating),
            WaitingForUser => matches!(
                to,
                Running | WaitingForExternalResource | Completed | Failed | Cancelled | Unknown
            ),
            WaitingForExternalResource => {
                matches!(
                    to,
                    Running | WaitingForUser | Blocked | Completed | Failed | Cancelled | Unknown
                )
            }
            Blocked => matches!(
                to,
                Running | WaitingForUser | Completed | Failed | Cancelled | Unknown
            ),
            Unknown => matches!(
                to,
                Running
                    | WaitingForUser
                    | WaitingForExternalResource
                    | Completed
                    | Failed
                    | Cancelled
            ),
        }
    }

    pub fn transition(self, to: RunState) -> Result<RunState, TransitionError> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(TransitionError { from: self, to })
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RunState::Created => "Starting",
            RunState::Estimating => "Estimating",
            RunState::Running => "Running",
            RunState::WaitingForUser => "Waiting for you",
            RunState::WaitingForExternalResource => "Waiting on external resource",
            RunState::Blocked => "Blocked",
            RunState::Completed => "Run finished",
            RunState::Failed => "Failed",
            RunState::Cancelled => "Cancelled",
            RunState::Unknown => "Status unknown",
        }
    }
}

/// Why a run ended or became unresolved. Stored as a short code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ExitReason {
    /// The agent reported that its turn finished.
    AgentFinished,
    /// The agent reported an error ending the turn.
    AgentError,
    /// The agent reported the turn was aborted.
    AgentAborted,
    /// A tool failure flagged as a user interrupt.
    UserInterrupt,
    /// A new prompt arrived in the same session before this run reported an end.
    Superseded,
    /// The agent went idle without reporting the end of this turn.
    IdleWithoutCompletion,
    /// The vendor session ended while this run was open.
    SessionEnded,
    /// The wrapped process exited.
    ProcessExited,
    /// No events arrived for a long time.
    Stale,
    /// The collector restarted while this run was open.
    CollectorRestarted,
}

impl ExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::AgentFinished => "agent_finished",
            ExitReason::AgentError => "agent_error",
            ExitReason::AgentAborted => "agent_aborted",
            ExitReason::UserInterrupt => "user_interrupt",
            ExitReason::Superseded => "superseded",
            ExitReason::IdleWithoutCompletion => "idle_without_completion",
            ExitReason::SessionEnded => "session_ended",
            ExitReason::ProcessExited => "process_exited",
            ExitReason::Stale => "stale",
            ExitReason::CollectorRestarted => "collector_restarted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::RunState::*;
    use super::*;

    #[test]
    fn terminal_states_are_final() {
        for from in [Completed, Failed, Cancelled] {
            for to in RunState::ALL {
                if to != from {
                    assert!(
                        !from.can_transition_to(to),
                        "{from:?} -> {to:?} must be illegal"
                    );
                }
            }
        }
    }

    #[test]
    fn waiting_can_recover() {
        assert_eq!(Running.transition(WaitingForUser), Ok(WaitingForUser));
        assert_eq!(WaitingForUser.transition(Running), Ok(Running));
        assert_eq!(WaitingForExternalResource.transition(Running), Ok(Running));
    }

    #[test]
    fn unknown_can_resolve_but_not_restart() {
        assert!(Unknown.can_transition_to(Running));
        assert!(Unknown.can_transition_to(Completed));
        assert!(!Unknown.can_transition_to(Created));
        assert!(!Unknown.can_transition_to(Estimating));
    }

    #[test]
    fn cannot_go_backwards_to_created() {
        for s in RunState::ALL {
            if s != Created {
                assert!(!s.can_transition_to(Created));
            }
        }
    }

    #[test]
    fn every_open_state_can_reach_every_terminal_state() {
        for s in RunState::ALL.into_iter().filter(|s| s.is_open()) {
            for t in [Completed, Failed, Cancelled] {
                assert!(s.can_transition_to(t), "{s:?} -> {t:?}");
            }
        }
    }

    #[test]
    fn string_forms_roundtrip() {
        for s in RunState::ALL {
            assert_eq!(RunState::parse(s.as_str()), Some(s));
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.as_str()));
        }
    }
}
