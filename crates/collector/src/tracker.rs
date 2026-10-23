//! In-memory state for one open run: the persisted row, its event timeline
//! and the signals read for it. Rebuilt from the database after restarts.

use crate::learning;
use crate::probe::{DiffStat, TranscriptCursor};
use crate::stuck::{FailureStreak, Stuck};
use chrono::{DateTime, Utc};
use runmarq_core::api::{AttentionKind, RunView};
use runmarq_core::event::{AgentProcess, PlanSnapshot};
use runmarq_core::forecast::EtaForecast;
use runmarq_core::run::RunState;
use runmarq_core::signals::RunSignals;
use runmarq_estimator::timeline::{RunSnapshot, Timeline, TimelineEvent};
use runmarq_storage::RunRow;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Matches the largest plan an event may carry.
const MAX_TASKS: usize = 500;

#[derive(Debug, Clone)]
pub struct Tracker {
    pub row: RunRow,
    pub timeline: Timeline,
    /// Start times of tool calls in flight, by tool use id.
    pub tool_starts: HashMap<String, DateTime<Utc>>,
    /// Start of the current waiting period, if waiting.
    pub waiting_since: Option<DateTime<Utc>>,
    /// Completed waiting periods, in milliseconds.
    pub waiting_ms: u64,
    pub attention: Option<AttentionKind>,
    /// Number of approval requests so far (makes attention notifications unique).
    pub approvals: u32,
    pub failures: FailureStreak,
    /// The current stuck episode, and how many there have been.
    pub stuck: Option<Stuck>,
    pub stuck_episodes: u32,
    pub last_forecast: Option<EtaForecast>,
    pub last_persisted: Option<EtaForecast>,
    pub launcher_id: Option<String>,
    pub agent_process: Option<AgentProcess>,
    /// Tasks from agents that edit their list a task at a time, and whether
    /// each is done.
    pub tasks: BTreeMap<String, bool>,
    pub signals: RunSignals,
    pub last_signal_snapshot_ms: Option<u64>,
    /// Paths reported by the agent's hooks. Held in memory only.
    pub transcript_path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// Taken while a probe runs, then returned.
    pub transcript: Option<TranscriptCursor>,
    pub git_baseline: Option<DiffStat>,
    pub last_git_probe: Option<DateTime<Utc>>,
    pub probing: bool,
}

impl Tracker {
    pub fn new(row: RunRow) -> Tracker {
        let waiting_ms = row.waiting_duration_ms.unwrap_or(0);
        let signals = row.signals.clone().unwrap_or_default();
        Tracker {
            timeline: Timeline::new(row.started_at, Vec::new()),
            row,
            tool_starts: HashMap::new(),
            waiting_since: None,
            waiting_ms,
            attention: None,
            approvals: 0,
            failures: FailureStreak::default(),
            stuck: None,
            stuck_episodes: 0,
            last_forecast: None,
            last_persisted: None,
            launcher_id: None,
            agent_process: None,
            tasks: BTreeMap::new(),
            signals,
            last_signal_snapshot_ms: None,
            transcript_path: None,
            cwd: None,
            transcript: None,
            git_baseline: None,
            last_git_probe: None,
            probing: false,
        }
    }

    pub fn state(&self) -> RunState {
        self.row.status
    }

    fn ms_between(a: DateTime<Utc>, b: DateTime<Utc>) -> u64 {
        (b - a).num_milliseconds().max(0) as u64
    }

    /// Wall-clock milliseconds from start to `now` (or to the end).
    pub fn wall_ms(&self, now: DateTime<Utc>) -> u64 {
        Self::ms_between(self.row.started_at, self.row.ended_at.unwrap_or(now))
    }

    pub fn total_waiting_ms(&self, now: DateTime<Utc>) -> u64 {
        let current = self
            .waiting_since
            .map(|s| Self::ms_between(s, self.row.ended_at.unwrap_or(now)))
            .unwrap_or(0);
        self.waiting_ms + current
    }

    pub fn active_ms(&self, now: DateTime<Utc>) -> u64 {
        self.wall_ms(now).saturating_sub(self.total_waiting_ms(now))
    }

    pub fn begin_waiting(&mut self, at: DateTime<Utc>) {
        if self.waiting_since.is_none() {
            self.waiting_since = Some(at.max(self.row.started_at));
        }
    }

    pub fn end_waiting(&mut self, at: DateTime<Utc>) {
        if let Some(since) = self.waiting_since.take() {
            self.waiting_ms += Self::ms_between(since, at);
        }
        if self.attention == Some(AttentionKind::ApprovalRequired) {
            self.attention = None;
        }
    }

    /// Adds an event to the timeline and refreshes the counters derived from it.
    pub fn record(&mut self, e: TimelineEvent) {
        if self.timeline.len() >= learning::MAX_TIMELINE_EVENTS {
            return;
        }
        let at = e.at;
        self.timeline.push(e);
        let s = self.timeline.snapshot(at.max(self.row.last_event_at));
        self.row.tool_calls = s.tool_calls;
        self.row.tool_failures = s.tool_failures;
        self.row.test_runs = s.test_runs;
    }

    /// The run as the estimator sees it at `now`.
    pub fn snapshot(&self, now: DateTime<Utc>) -> RunSnapshot {
        let mut s = self.timeline.snapshot(now);
        s.active_elapsed = self.active_ms(now) as f64 / 1000.0;
        if s.plan.is_none() {
            s.plan = self.plan();
        }
        s
    }

    /// Applies one task change and returns the plan it leaves. Tasks created
    /// in an earlier run of the session join the list when they are updated.
    pub fn apply_task(&mut self, id: &str, status: Option<&str>) -> Option<PlanSnapshot> {
        if status == Some("deleted") {
            self.tasks.remove(id);
        } else if self.tasks.len() < MAX_TASKS || self.tasks.contains_key(id) {
            self.tasks
                .insert(id.to_string(), status == Some("completed"));
        }
        (!self.tasks.is_empty()).then(|| PlanSnapshot {
            total_steps: self.tasks.len() as u32,
            completed_steps: self.tasks.values().filter(|done| **done).count() as u32,
        })
    }

    pub fn plan(&self) -> Option<PlanSnapshot> {
        match (self.row.plan_total, self.row.plan_completed) {
            (Some(t), Some(c)) => Some(PlanSnapshot {
                total_steps: t,
                completed_steps: c,
            }),
            _ => None,
        }
    }

    /// Writes live accounting into the row before persisting.
    pub fn sync_row(&mut self, now: DateTime<Utc>) {
        let wall = self.wall_ms(now);
        let waiting = self.total_waiting_ms(now);
        self.row.waiting_duration_ms = Some(waiting);
        if self.row.status.is_terminal() {
            self.row.wall_duration_ms = Some(wall);
            self.row.active_duration_ms = Some(wall.saturating_sub(waiting));
        } else {
            self.row.wall_duration_ms = None;
            self.row.active_duration_ms = None;
        }
        let empty = self.signals == RunSignals::default();
        self.row.signals = (!empty).then(|| self.signals.clone());
    }

    pub fn view(&self, now: DateTime<Utc>, repository_name: Option<String>) -> RunView {
        to_view(&self.row, now, repository_name, Some(self))
    }
}

pub fn to_view(
    row: &RunRow,
    now: DateTime<Utc>,
    repository_name: Option<String>,
    live: Option<&Tracker>,
) -> RunView {
    let wall = live
        .map(|t| t.wall_ms(now))
        .or(row.wall_duration_ms)
        .unwrap_or_else(|| {
            (row.ended_at.unwrap_or(now) - row.started_at)
                .num_milliseconds()
                .max(0) as u64
        });
    let waiting = live
        .map(|t| t.total_waiting_ms(now))
        .or(row.waiting_duration_ms)
        .unwrap_or(0);
    let secs = |ms: Option<u64>| ms.map(|v| (v / 1000) as u32);
    let terminal = row.status.is_terminal();
    let signals = match live {
        Some(t) if t.signals != RunSignals::default() => Some(t.signals.clone()),
        _ => row.signals.clone(),
    };
    RunView {
        id: row.id.clone(),
        session_id: row.session_id.clone(),
        agent: row.agent,
        repository_id: row.repository_id.clone(),
        repository_name,
        title: row.title.clone(),
        task_category: row.task_category,
        prompt_known: row.prompt_features.is_some(),
        model: row.model.clone(),
        effort: row.effort.clone(),
        state: row.status,
        exit_reason: row.exit_reason.clone(),
        started_at: row.started_at,
        ended_at: row.ended_at,
        last_event_at: row.last_event_at,
        wall_ms: wall,
        active_ms: row
            .active_duration_ms
            .unwrap_or(wall.saturating_sub(waiting)),
        waiting_ms: waiting,
        current_activity: if terminal { None } else { row.current_activity },
        current_tool: if terminal {
            None
        } else {
            row.current_tool.clone()
        },
        phase: live
            .filter(|t| !terminal && !t.timeline.is_empty())
            .map(|t| t.timeline.snapshot(now).phase),
        attention: live.and_then(|t| t.attention),
        attention_detail: live
            .filter(|t| t.attention == Some(AttentionKind::Stuck))
            .and_then(|t| t.stuck.as_ref().map(|s| s.detail.clone())),
        tool_calls: row.tool_calls,
        tool_failures: row.tool_failures,
        plan: match (row.plan_total, row.plan_completed) {
            (Some(t), Some(c)) => Some(PlanSnapshot {
                total_steps: t,
                completed_steps: c,
            }),
            _ => None,
        },
        signals,
        forecast: live
            .and_then(|t| t.last_forecast.clone())
            .filter(|_| !terminal),
        initial_p10_seconds: secs(row.initial_p10_ms),
        initial_p50_seconds: secs(row.initial_p50_ms),
        initial_p90_seconds: secs(row.initial_p90_ms),
    }
}
