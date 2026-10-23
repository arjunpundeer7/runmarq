//! Stuck detection: flags a working run when there is clear evidence that it
//! is not making progress. Deliberately conservative, because a slow run is
//! not a stuck one. [`detect`] is a pure function of [`Evidence`], which the
//! engine gathers from a run's tracker on every tick.
//!
//! The rules, in order:
//! - the same tool (or, for shell tools, the same command) failed
//!   [`REPEATED_FAILURES`] times in a row;
//! - one tool call has been running far longer than that command's learned
//!   duration, or far longer than tool calls usually take, and never less
//!   than [`TOOL_FLOOR_SECS`]; subagents and tools that may be waiting on
//!   the user are exempt;
//! - the run is well past its initial 90th-percentile estimate and nothing
//!   has happened for [`QUIET_SECS`].

use crate::tracker::Tracker;
use chrono::{DateTime, Utc};
use runmarq_core::signals::Phase;
use runmarq_estimator::command::{CommandStat, CommandTable};
use std::collections::HashMap;

/// Consecutive failures of the same tool that count as stuck.
pub const REPEATED_FAILURES: u32 = 3;
/// No tool call counts as stuck before this, however quick it usually is.
pub const TOOL_FLOOR_SECS: f64 = 5.0 * 60.0;
/// A command with a learned duration: this many times its 90th percentile.
pub const LEARNED_FACTOR: f64 = 3.0;
/// A tool call without a learned duration. Most take seconds.
pub const UNLEARNED_TOOL_SECS: f64 = 10.0 * 60.0;
/// Past the estimate: at least this multiple of the predicted 90th percentile,
pub const PAST_ESTIMATE_FACTOR: f64 = 2.0;
/// and at least this far past it,
pub const PAST_ESTIMATE_MARGIN_SECS: f64 = 10.0 * 60.0;
/// with no hook event or transcript turn for this long.
pub const QUIET_SECS: f64 = 5.0 * 60.0;

/// z-score of the 90th percentile of a normal distribution.
const Z90: f64 = 1.281_55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckReason {
    RepeatedFailures,
    LongToolCall,
    PastEstimate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stuck {
    pub reason: StuckReason,
    /// Short and factual, for example "Bash failed 3 times in a row". Names
    /// tools only, never commands or arguments.
    pub detail: String,
}

/// Consecutive failures of one tool, or of one command for shell tools.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FailureStreak {
    key: Option<String>,
    pub tool: Option<String>,
    pub same_command: bool,
    pub count: u32,
}

impl FailureStreak {
    /// Records a finished tool call. Any success ends the streak; a failure
    /// of a different tool starts a new one.
    pub fn record(&mut self, tool: Option<&str>, command_key: Option<&str>, failed: bool) {
        if !failed {
            *self = FailureStreak::default();
            return;
        }
        let key = command_key.or(tool).map(String::from);
        if key.is_none() || key != self.key {
            *self = FailureStreak {
                key,
                tool: tool.map(String::from),
                same_command: command_key.is_some(),
                count: 0,
            };
        }
        self.count += 1;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolInFlight {
    pub tool: Option<String>,
    /// Active seconds since the call started.
    pub elapsed_secs: f64,
    /// The 90th percentile of this command's learned duration, if known.
    pub usual_p90_secs: Option<f64>,
    /// The call may legitimately last any time: a subagent, or a tool that
    /// may be waiting on the user, such as a question to answer.
    pub open_ended: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Evidence {
    pub failures: FailureStreak,
    pub in_flight: Option<ToolInFlight>,
    /// Active seconds since the run started.
    pub active_secs: f64,
    /// Total duration the first estimate put at the 90th percentile.
    pub initial_p90_secs: Option<f64>,
    /// Seconds since the last hook event or transcript turn.
    pub quiet_secs: f64,
}

/// "40 s", "12 min", "1 h 5 min".
fn human(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    match s {
        0..=89 => format!("{s} s"),
        90..=7199 => format!("{} min", (s + 30) / 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

pub fn detect(e: &Evidence) -> Option<Stuck> {
    let f = &e.failures;
    if f.count >= REPEATED_FAILURES {
        let what = match (&f.tool, f.same_command) {
            (_, true) => "The same command".to_string(),
            (Some(t), false) => t.clone(),
            (None, false) => "The same tool".to_string(),
        };
        return Some(Stuck {
            reason: StuckReason::RepeatedFailures,
            detail: format!("{what} failed {} times in a row", f.count),
        });
    }
    if let Some(t) = &e.in_flight {
        let limit = match t.usual_p90_secs {
            Some(p90) => (LEARNED_FACTOR * p90).max(TOOL_FLOOR_SECS),
            None => UNLEARNED_TOOL_SECS,
        };
        if t.open_ended || t.elapsed_secs < limit {
            // A tool still within its time explains any silence.
            return None;
        }
        let what = t.tool.clone().unwrap_or_else(|| "A tool call".into());
        let running = human(t.elapsed_secs);
        let detail = match t.usual_p90_secs {
            Some(p90) => format!(
                "{what} has been running for {running}; it usually finishes within {}",
                human(p90)
            ),
            None => format!("{what} has been running for {running}"),
        };
        return Some(Stuck {
            reason: StuckReason::LongToolCall,
            detail,
        });
    }
    let p90 = e.initial_p90_secs?;
    let limit = (PAST_ESTIMATE_FACTOR * p90).max(p90 + PAST_ESTIMATE_MARGIN_SECS);
    (e.active_secs >= limit && e.quiet_secs >= QUIET_SECS).then(|| Stuck {
        reason: StuckReason::PastEstimate,
        detail: format!(
            "No activity for {}, well past the estimate",
            human(e.quiet_secs)
        ),
    })
}

/// The 90th percentile of a command's learned duration, preferring this
/// repository's timings, as the estimator does.
fn usual_p90(
    commands: &HashMap<(String, String), CommandStat>,
    repository_id: Option<&str>,
    key: &str,
) -> Option<f64> {
    let table = CommandTable::new(commands.values().filter(|c| c.command_key == key).cloned());
    let d = table.lookup(repository_id, key)?;
    Some((d.mu + Z90 * d.sigma).exp())
}

/// What the tracker shows about the run's progress at `now`.
pub fn evidence(
    t: &Tracker,
    commands: &HashMap<(String, String), CommandStat>,
    now: DateTime<Utc>,
) -> Evidence {
    let snap = t.snapshot(now);
    let in_flight = snap.in_flight.map(|f| ToolInFlight {
        tool: t.row.current_tool.clone(),
        usual_p90_secs: f
            .command_key
            .as_deref()
            .filter(|_| f.elapsed >= TOOL_FLOOR_SECS)
            .and_then(|k| usual_p90(commands, t.row.repository_id.as_deref(), k)),
        elapsed_secs: f.elapsed,
        // Shell commands carry a command key; other tools runmarq cannot
        // classify land in the execute phase without one.
        open_ended: f.phase == Phase::Delegate
            || (f.phase == Phase::Execute && f.command_key.is_none()),
    });
    let last = t.signals.last_turn_at.map_or(t.row.last_event_at, |turn| {
        turn.min(now).max(t.row.last_event_at)
    });
    Evidence {
        failures: t.failures.clone(),
        in_flight,
        active_secs: snap.active_elapsed,
        initial_p90_secs: t.row.initial_p90_ms.map(|ms| ms as f64 / 1000.0),
        quiet_secs: (now - last).num_milliseconds().max(0) as f64 / 1000.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: f64 = 60.0;

    fn failing(n: u32, tool: &str, command: Option<&str>) -> FailureStreak {
        let mut s = FailureStreak::default();
        for _ in 0..n {
            s.record(Some(tool), command, true);
        }
        s
    }

    fn tool(elapsed: f64, usual_p90: Option<f64>) -> Option<ToolInFlight> {
        Some(ToolInFlight {
            tool: Some("Bash".into()),
            elapsed_secs: elapsed,
            usual_p90_secs: usual_p90,
            open_ended: false,
        })
    }

    #[test]
    fn streaks_count_only_the_same_tool_in_a_row() {
        let mut s = failing(2, "Edit", None);
        assert_eq!(s.count, 2);
        s.record(Some("Bash"), Some("cmd_a"), true);
        assert_eq!(
            (s.count, s.same_command),
            (1, true),
            "a new tool starts over"
        );
        s.record(Some("Bash"), Some("cmd_b"), true);
        assert_eq!(s.count, 1, "another command is not the same one");
        s.record(Some("Bash"), Some("cmd_b"), true);
        assert_eq!(s.count, 2);
        s.record(Some("Read"), None, false);
        assert_eq!(s, FailureStreak::default(), "any success ends it");
        for _ in 0..5 {
            s.record(None, None, true);
        }
        assert_eq!(s.count, 1, "unnamed failures cannot be matched");
    }

    #[test]
    fn repeated_failures() {
        let mut e = Evidence {
            failures: failing(2, "Edit", None),
            ..Default::default()
        };
        assert_eq!(detect(&e), None);
        e.failures = failing(3, "Edit", None);
        let s = detect(&e).unwrap();
        assert_eq!(s.reason, StuckReason::RepeatedFailures);
        assert_eq!(s.detail, "Edit failed 3 times in a row");
        e.failures = failing(4, "Bash", Some("cmd_0123"));
        assert_eq!(
            detect(&e).unwrap().detail,
            "The same command failed 4 times in a row"
        );
    }

    #[test]
    fn unlearned_tool_calls_get_ten_minutes() {
        let mut e = Evidence {
            in_flight: tool(9.0 * MIN, None),
            ..Default::default()
        };
        assert_eq!(detect(&e), None);
        e.in_flight = tool(10.0 * MIN, None);
        let s = detect(&e).unwrap();
        assert_eq!(s.reason, StuckReason::LongToolCall);
        assert_eq!(s.detail, "Bash has been running for 10 min");
    }

    #[test]
    fn open_ended_calls_are_never_stuck_and_explain_the_silence() {
        let e = Evidence {
            in_flight: Some(ToolInFlight {
                tool: Some("AskUserQuestion".into()),
                elapsed_secs: 90.0 * MIN,
                usual_p90_secs: None,
                open_ended: true,
            }),
            active_secs: 95.0 * MIN,
            initial_p90_secs: Some(10.0 * MIN),
            quiet_secs: 90.0 * MIN,
            ..Default::default()
        };
        assert_eq!(detect(&e), None);
    }

    #[test]
    fn learned_commands_use_their_own_duration_with_a_floor() {
        // Usually within 30 s: never flagged before the five-minute floor.
        let mut e = Evidence {
            in_flight: tool(4.0 * MIN, Some(30.0)),
            ..Default::default()
        };
        assert_eq!(detect(&e), None);
        e.in_flight = tool(5.0 * MIN, Some(30.0));
        assert_eq!(
            detect(&e).unwrap().detail,
            "Bash has been running for 5 min; it usually finishes within 30 s"
        );
        // Usually within 6 min: a long build gets three times that.
        e.in_flight = tool(15.0 * MIN, Some(6.0 * MIN));
        assert_eq!(detect(&e), None);
        e.in_flight = tool(18.0 * MIN, Some(6.0 * MIN));
        assert!(detect(&e).is_some());
    }

    #[test]
    fn past_the_estimate_and_quiet() {
        let mut e = Evidence {
            active_secs: 25.0 * MIN,
            initial_p90_secs: Some(10.0 * MIN),
            quiet_secs: 6.0 * MIN,
            ..Default::default()
        };
        let s = detect(&e).unwrap();
        assert_eq!(s.reason, StuckReason::PastEstimate);
        assert_eq!(s.detail, "No activity for 6 min, well past the estimate");

        e.quiet_secs = 2.0 * MIN;
        assert_eq!(detect(&e), None, "recent activity is progress");
        e.quiet_secs = 6.0 * MIN;
        e.active_secs = 15.0 * MIN;
        assert_eq!(detect(&e), None, "not far enough past the estimate");
        e.active_secs = 25.0 * MIN;
        e.in_flight = tool(6.0 * MIN, None);
        assert_eq!(
            detect(&e),
            None,
            "a tool within its time explains the silence"
        );
        e.in_flight = None;
        e.initial_p90_secs = None;
        assert_eq!(detect(&e), None, "no estimate, no comparison");
    }

    #[test]
    fn short_estimates_still_need_a_margin() {
        let e = Evidence {
            active_secs: 8.0 * MIN,
            initial_p90_secs: Some(2.0 * MIN),
            quiet_secs: 6.0 * MIN,
            ..Default::default()
        };
        assert_eq!(detect(&e), None);
        let e = Evidence {
            active_secs: 12.0 * MIN,
            ..e
        };
        assert!(detect(&e).is_some());
    }

    #[test]
    fn learned_p90_prefers_the_repository() {
        let mut commands = HashMap::new();
        for (repo, secs) in [("r1", 100.0), ("r2", 10.0)] {
            let mut s = CommandStat::new(repo, "cmd_t");
            for _ in 0..5 {
                s.observe(secs, false);
            }
            commands.insert((repo.to_string(), "cmd_t".to_string()), s);
        }
        let local = usual_p90(&commands, Some("r1"), "cmd_t").unwrap();
        assert!(local > 100.0 && local < 300.0, "{local}");
        let pooled = usual_p90(&commands, Some("elsewhere"), "cmd_t").unwrap();
        assert!(pooled < local);
        assert_eq!(usual_p90(&commands, Some("r1"), "cmd_other"), None);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human(40.0), "40 s");
        assert_eq!(human(12.0 * MIN), "12 min");
        assert_eq!(human(65.0 * MIN), "65 min");
        assert_eq!(human(125.0 * MIN), "2 h 5 min");
    }
}
