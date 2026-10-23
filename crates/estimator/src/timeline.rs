//! A run as a sequence of events, replayed into active-time segments.
//!
//! Live forecasting and training both derive their inputs from
//! [`Timeline::replay`], so a feature means the same thing in both places.

use chrono::{DateTime, Utc};
use runmarq_core::event::{ActivityCategory, EventType, PlanSnapshot};
use runmarq_core::signals::Phase;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEvent {
    pub at: DateTime<Utc>,
    pub kind: EventType,
    pub activity: Option<ActivityCategory>,
    pub tool_use_id: Option<String>,
    pub command_key: Option<String>,
    pub duration_ms: Option<u64>,
    pub plan: Option<PlanSnapshot>,
    pub subagent_id: Option<String>,
}

impl TimelineEvent {
    pub fn new(at: DateTime<Utc>, kind: EventType) -> TimelineEvent {
        TimelineEvent {
            at,
            kind,
            activity: None,
            tool_use_id: None,
            command_key: None,
            duration_ms: None,
            plan: None,
            subagent_id: None,
        }
    }
}

/// A closed stretch of a run: one tool call and the model time after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub phase: Phase,
    pub start: f64,
    pub end: f64,
    /// Active seconds the tool itself took.
    pub tool_secs: f64,
    pub failed: bool,
    pub command_key: Option<String>,
}

impl Segment {
    pub fn gap_secs(&self) -> f64 {
        (self.end - self.start - self.tool_secs).max(0.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InFlight {
    pub phase: Phase,
    pub elapsed: f64,
    pub command_key: Option<String>,
}

/// Everything known about a run at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSnapshot {
    pub active_elapsed: f64,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub failure_streak: u32,
    pub test_runs: u32,
    pub test_failure_streak: u32,
    pub last_test_passed: bool,
    pub edits_since_last_test: u32,
    pub active_subagents: u32,
    pub plan: Option<PlanSnapshot>,
    pub phase: Phase,
    pub segment_elapsed: f64,
    pub in_flight: Option<InFlight>,
    /// Active seconds since the current segment's tool finished.
    pub since_tool_end: Option<f64>,
    pub last_verify_failed: Option<bool>,
    pub segments: u32,
    pub verify_failures: u32,
    pub compactions: u32,
}

impl Default for RunSnapshot {
    fn default() -> Self {
        RunSnapshot {
            active_elapsed: 0.0,
            tool_calls: 0,
            tool_failures: 0,
            failure_streak: 0,
            test_runs: 0,
            test_failure_streak: 0,
            last_test_passed: false,
            edits_since_last_test: 0,
            active_subagents: 0,
            plan: None,
            phase: Phase::Plan,
            segment_elapsed: 0.0,
            in_flight: None,
            since_tool_end: None,
            last_verify_failed: None,
            segments: 0,
            verify_failures: 0,
            compactions: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Timeline {
    pub start: DateTime<Utc>,
    events: Vec<TimelineEvent>,
    /// Waiting intervals in wall seconds from start; `end` is `None` while open.
    waits: Vec<(f64, Option<f64>)>,
}

fn resumes(kind: EventType) -> bool {
    use EventType::*;
    matches!(
        kind,
        ToolStarted
            | ToolCompleted
            | ToolFailed
            | SubagentStarted
            | SubagentCompleted
            | PlanUpdated
            | ContextCompacting
            | ApprovalResolved
            | RunCompleted
            | RunFailed
            | RunCancelled
    )
}

#[derive(Debug, Clone)]
struct OpenSegment {
    phase: Phase,
    start: f64,
    tool_start: Option<f64>,
    tool_end: Option<f64>,
    tool_id: Option<String>,
    failed: bool,
    command_key: Option<String>,
}

impl OpenSegment {
    fn close(self, end: f64) -> Segment {
        let end = end.max(self.start);
        let tool_secs = match (self.tool_start, self.tool_end) {
            (Some(s), Some(e)) => (e - s).max(0.0),
            (Some(s), None) => (end - s).max(0.0),
            _ => 0.0,
        };
        Segment {
            phase: self.phase,
            start: self.start,
            end,
            tool_secs: tool_secs.min(end - self.start),
            failed: self.failed,
            command_key: self.command_key,
        }
    }
}

/// The result of replaying a timeline up to a moment.
#[derive(Debug, Clone)]
pub struct Replay {
    pub closed: Vec<Segment>,
    pub snapshot: RunSnapshot,
    current: OpenSegment,
}

impl Timeline {
    pub fn new(start: DateTime<Utc>, mut events: Vec<TimelineEvent>) -> Timeline {
        events.sort_by_key(|e| e.at);
        let mut t = Timeline {
            start,
            events: Vec::new(),
            waits: Vec::new(),
        };
        for e in &events {
            t.track_wait(e);
        }
        t.events = events;
        t
    }

    fn track_wait(&mut self, e: &TimelineEvent) {
        let w = Self::wall(self.start, e.at);
        if e.kind == EventType::ApprovalRequested {
            if self.waits.last().is_none_or(|(_, end)| end.is_some()) {
                self.waits.push((w, None));
            }
        } else if resumes(e.kind) {
            if let Some(last) = self.waits.last_mut() {
                if last.1.is_none() {
                    last.1 = Some(w.max(last.0));
                }
            }
        }
    }

    /// Adds an event; events usually arrive in order, which keeps this cheap.
    pub fn push(&mut self, e: TimelineEvent) {
        if self.events.last().is_none_or(|last| last.at <= e.at) {
            self.track_wait(&e);
            self.events.push(e);
        } else {
            let mut events = std::mem::take(&mut self.events);
            events.push(e);
            *self = Timeline::new(self.start, events);
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The earliest moment at which `active` seconds of active time had passed.
    pub fn at_active(&self, active: f64) -> DateTime<Utc> {
        let mut w = active.max(0.0);
        for &(s, e) in &self.waits {
            if s >= w {
                break;
            }
            match e {
                Some(e) => w += e - s,
                None => {
                    w = s;
                    break;
                }
            }
        }
        self.start + chrono::Duration::milliseconds((w * 1000.0).round() as i64)
    }

    fn wall(start: DateTime<Utc>, t: DateTime<Utc>) -> f64 {
        ((t - start).num_milliseconds() as f64 / 1000.0).max(0.0)
    }

    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    /// Active (non-waiting) seconds from the start of the run to `t`.
    pub fn active_at(&self, t: DateTime<Utc>) -> f64 {
        let w = Self::wall(self.start, t);
        let waiting: f64 = self
            .waits
            .iter()
            .map(|(s, e)| {
                let end = e.unwrap_or(f64::INFINITY).min(w);
                (end - s).max(0.0)
            })
            .sum();
        (w - waiting).max(0.0)
    }

    pub fn replay(&self, until: DateTime<Utc>) -> Replay {
        let mut snap = RunSnapshot::default();
        let mut closed: Vec<Segment> = Vec::new();
        let mut cur = OpenSegment {
            phase: Phase::Plan,
            start: 0.0,
            tool_start: None,
            tool_end: None,
            tool_id: None,
            failed: false,
            command_key: None,
        };
        let mut open_ids: HashSet<String> = HashSet::new();
        let mut subagents: HashSet<String> = HashSet::new();
        let mut anonymous_subagents: u32 = 0;

        for e in self.events.iter().filter(|e| e.at <= until) {
            let a = self.active_at(e.at);
            match e.kind {
                EventType::ToolStarted => {
                    snap.tool_calls += 1;
                    let phase = Phase::of(e.activity.unwrap_or(ActivityCategory::Unknown));
                    let next = OpenSegment {
                        phase,
                        start: a,
                        tool_start: Some(a),
                        tool_end: None,
                        tool_id: e.tool_use_id.clone(),
                        failed: false,
                        command_key: e.command_key.clone(),
                    };
                    closed.push(std::mem::replace(&mut cur, next).close(a));
                    if let Some(id) = &e.tool_use_id {
                        open_ids.insert(id.clone());
                    }
                }
                EventType::ToolCompleted | EventType::ToolFailed => {
                    let failed = e.kind == EventType::ToolFailed;
                    let activity = e.activity.unwrap_or(ActivityCategory::Unknown);
                    let phase = Phase::of(activity);
                    let had_start = e.tool_use_id.as_ref().is_some_and(|id| open_ids.remove(id));
                    let is_current = cur.tool_end.is_none()
                        && cur.tool_start.is_some()
                        && match (&cur.tool_id, &e.tool_use_id) {
                            (Some(a), Some(b)) => a == b,
                            (None, _) | (_, None) => cur.phase == phase,
                        };
                    if is_current {
                        cur.tool_end = Some(a);
                        cur.failed = failed;
                        if cur.command_key.is_none() {
                            cur.command_key = e.command_key.clone();
                        }
                    } else if !had_start {
                        // Completion-only adapters report a tool once, when it ends.
                        snap.tool_calls += 1;
                        let dur = e.duration_ms.map(|d| d as f64 / 1000.0).unwrap_or(0.0);
                        let tool_start = (a - dur).max(cur.start);
                        let next = OpenSegment {
                            phase,
                            start: tool_start,
                            tool_start: Some(tool_start),
                            tool_end: Some(a),
                            tool_id: e.tool_use_id.clone(),
                            failed,
                            command_key: e.command_key.clone(),
                        };
                        closed.push(std::mem::replace(&mut cur, next).close(tool_start));
                    }
                    if failed {
                        snap.tool_failures += 1;
                        snap.failure_streak += 1;
                    } else {
                        snap.failure_streak = 0;
                    }
                    match activity {
                        ActivityCategory::RunningTests | ActivityCategory::Building => {
                            if activity == ActivityCategory::RunningTests {
                                snap.test_runs += 1;
                                if failed {
                                    snap.test_failure_streak += 1;
                                } else {
                                    snap.test_failure_streak = 0;
                                }
                                snap.last_test_passed = !failed;
                                snap.edits_since_last_test = 0;
                            }
                            snap.last_verify_failed = Some(failed);
                            if failed {
                                snap.verify_failures += 1;
                            }
                        }
                        ActivityCategory::EditingCode if !failed => snap.edits_since_last_test += 1,
                        _ => {}
                    }
                }
                EventType::PlanUpdated => {
                    if e.plan.is_some() {
                        snap.plan = e.plan.clone();
                    }
                }
                EventType::SubagentStarted => match &e.subagent_id {
                    Some(id) => {
                        subagents.insert(id.clone());
                    }
                    None => anonymous_subagents += 1,
                },
                EventType::SubagentCompleted => {
                    let removed = e
                        .subagent_id
                        .as_ref()
                        .is_some_and(|id| subagents.remove(id));
                    if !removed {
                        anonymous_subagents = anonymous_subagents.saturating_sub(1);
                    }
                }
                EventType::ContextCompacting => {
                    snap.compactions += 1;
                    let next = OpenSegment {
                        phase: Phase::Compact,
                        start: a,
                        tool_start: Some(a),
                        tool_end: Some(a),
                        tool_id: None,
                        failed: false,
                        command_key: None,
                    };
                    closed.push(std::mem::replace(&mut cur, next).close(a));
                }
                _ => {}
            }
        }

        let now = self.active_at(until);
        snap.active_elapsed = now;
        snap.active_subagents = subagents.len() as u32 + anonymous_subagents;
        snap.phase = cur.phase;
        snap.segment_elapsed = (now - cur.start).max(0.0);
        snap.in_flight = match (cur.tool_start, cur.tool_end) {
            (Some(s), None) => Some(InFlight {
                phase: cur.phase,
                elapsed: (now - s).max(0.0),
                command_key: cur.command_key.clone(),
            }),
            _ => None,
        };
        snap.since_tool_end = cur.tool_end.map(|e| (now - e).max(0.0));
        snap.segments = closed
            .iter()
            .filter(|s| s.end > s.start || s.phase != Phase::Plan)
            .count() as u32;
        Replay {
            closed,
            snapshot: snap,
            current: cur,
        }
    }

    pub fn snapshot(&self, at: DateTime<Utc>) -> RunSnapshot {
        self.replay(at).snapshot
    }

    /// All segments of a finished run, the last one closed at `end`.
    pub fn segments(&self, end: DateTime<Utc>) -> Vec<Segment> {
        let r = self.replay(end);
        let end_active = self.active_at(end);
        let mut out: Vec<Segment> = r
            .closed
            .into_iter()
            .filter(|s| s.end > s.start || s.tool_secs > 0.0)
            .collect();
        out.push(r.current.close(end_active));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn t(s: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::seconds(s)
    }

    fn tool(at: i64, kind: EventType, id: &str, a: ActivityCategory) -> TimelineEvent {
        TimelineEvent {
            activity: Some(a),
            tool_use_id: Some(id.into()),
            ..TimelineEvent::new(t(at), kind)
        }
    }

    fn sample() -> Timeline {
        use ActivityCategory::*;
        use EventType::*;
        Timeline::new(
            t(0),
            vec![
                tool(10, ToolStarted, "r", ReadingFiles),
                tool(11, ToolCompleted, "r", ReadingFiles),
                tool(20, ToolStarted, "e", EditingCode),
                tool(21, ToolCompleted, "e", EditingCode),
                tool(30, ToolStarted, "t1", RunningTests),
                tool(70, ToolFailed, "t1", RunningTests),
                TimelineEvent::new(t(80), ApprovalRequested),
                tool(100, ToolStarted, "t2", RunningTests),
                tool(130, ToolCompleted, "t2", RunningTests),
                TimelineEvent::new(t(140), RunCompleted),
            ],
        )
    }

    #[test]
    fn waiting_time_is_excluded() {
        let tl = sample();
        assert_eq!(tl.active_at(t(80)), 80.0);
        assert_eq!(tl.active_at(t(90)), 80.0, "waiting for approval");
        assert_eq!(tl.active_at(t(140)), 120.0);
    }

    #[test]
    fn segments_capture_phases_tool_time_and_outcomes() {
        let tl = sample();
        let segs = tl.segments(t(140));
        let phases: Vec<Phase> = segs.iter().map(|s| s.phase).collect();
        assert_eq!(
            phases,
            vec![
                Phase::Plan,
                Phase::Explore,
                Phase::Edit,
                Phase::Verify,
                Phase::Verify
            ]
        );
        assert_eq!(segs[0].end, 10.0);
        assert_eq!(segs[3].tool_secs, 40.0);
        assert!(segs[3].failed);
        assert_eq!(segs[3].end, 80.0, "the approval wait is not active time");
        assert_eq!(segs[4].tool_secs, 30.0);
        assert!(!segs[4].failed);
        assert_eq!(segs[4].end, 120.0);
        let total: f64 = segs.iter().map(|s| s.end - s.start).sum();
        assert_eq!(total, 120.0);
    }

    #[test]
    fn snapshot_mid_run() {
        let tl = sample();
        let s = tl.snapshot(t(50));
        assert_eq!(s.phase, Phase::Verify);
        assert_eq!(s.in_flight.as_ref().unwrap().elapsed, 20.0);
        assert_eq!(s.tool_calls, 3);
        assert_eq!(s.edits_since_last_test, 1);
        let s = tl.snapshot(t(75));
        assert!(s.in_flight.is_none());
        assert_eq!(s.since_tool_end, Some(5.0));
        assert_eq!(s.test_failure_streak, 1);
        assert_eq!(s.last_verify_failed, Some(true));
        let s = tl.snapshot(t(135));
        assert!(s.last_test_passed);
        assert_eq!(s.test_failure_streak, 0);
        assert_eq!(s.verify_failures, 1);
    }

    #[test]
    fn active_time_maps_back_to_wall_time() {
        let tl = sample();
        assert_eq!(tl.at_active(50.0), t(50));
        assert_eq!(tl.at_active(80.0), t(80));
        assert_eq!(tl.at_active(85.0), t(105));
        assert_eq!(tl.active_at(tl.at_active(110.0)), 110.0);
    }

    #[test]
    fn pushing_events_matches_building_at_once() {
        let all = sample();
        let mut live = Timeline::new(t(0), Vec::new());
        for e in all.events().iter().rev().take(3).rev() {
            live.push(e.clone());
        }
        for e in all.events().iter().take(all.len() - 3) {
            live.push(e.clone());
        }
        assert_eq!(live.events(), all.events());
        assert_eq!(live.active_at(t(140)), all.active_at(t(140)));
        assert_eq!(live.segments(t(140)), all.segments(t(140)));
    }

    #[test]
    fn completion_only_tools_use_reported_duration() {
        use ActivityCategory::*;
        let mut done = tool(50, EventType::ToolCompleted, "x", RunningTests);
        done.duration_ms = Some(20_000);
        let tl = Timeline::new(
            t(0),
            vec![done, TimelineEvent::new(t(60), EventType::RunCompleted)],
        );
        let segs = tl.segments(t(60));
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].end, 30.0);
        assert_eq!(segs[1].tool_secs, 20.0);
        assert_eq!(tl.snapshot(t(55)).tool_calls, 1);
    }
}
