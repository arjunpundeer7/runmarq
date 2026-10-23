//! Turns stored runs and events into estimator inputs. Live runs use the
//! same event conversion, so a timeline means the same thing in training
//! and in forecasting.

use chrono::{DateTime, Utc};
use runmarq_core::event::{EventEnvelope, EventType, PlanSnapshot};
use runmarq_core::run::{ExitReason, RunState};
use runmarq_estimator::command::CommandStat;
use runmarq_estimator::history::Outcome;
use runmarq_estimator::timeline::{Timeline, TimelineEvent};
use runmarq_estimator::training::{TrainingRun, TrainingSet};
use runmarq_storage::{CommandStatRow, EventRow, RunRow, SignalSnapshot, Store, StoreError};
use serde_json::Value;
use std::collections::HashMap;

/// Events kept per run timeline; longer runs keep their first events.
pub const MAX_TIMELINE_EVENTS: usize = 50_000;
/// A run cut off by a prompt this soon after its last event was still busy.
const STITCH_WITHIN: chrono::Duration = chrono::Duration::seconds(10);

/// Whether an event type matters to the run timeline.
pub fn on_timeline(kind: EventType) -> bool {
    !matches!(
        kind,
        EventType::SessionStarted
            | EventType::SessionEnded
            | EventType::AgentIdle
            | EventType::Heartbeat
    )
}

pub fn event_from_envelope(env: &EventEnvelope, at: DateTime<Utc>) -> TimelineEvent {
    let p = &env.payload;
    TimelineEvent {
        at,
        kind: env.event_type,
        activity: p.activity,
        tool_use_id: p.tool_use_id.clone(),
        command_key: p.command_key.clone(),
        duration_ms: p.duration_ms,
        plan: p.plan.clone(),
        subagent_id: p.subagent_id.clone(),
    }
}

fn text(meta: &Value, key: &str) -> Option<String> {
    meta.get(key).and_then(Value::as_str).map(String::from)
}

pub fn event_from_row(e: &EventRow) -> TimelineEvent {
    let m = &e.metadata;
    TimelineEvent {
        at: e.source_timestamp,
        kind: e.event_type,
        activity: e.activity,
        tool_use_id: text(m, "toolUseId"),
        command_key: text(m, "commandKey"),
        duration_ms: e.duration_ms,
        plan: m
            .get("plan")
            .and_then(|p| serde_json::from_value::<PlanSnapshot>(p.clone()).ok()),
        subagent_id: text(m, "subagentId"),
    }
}

pub fn timeline_from_rows(start: DateTime<Utc>, rows: &[EventRow]) -> Timeline {
    Timeline::new(
        start,
        rows.iter()
            .filter(|e| on_timeline(e.event_type))
            .take(MAX_TIMELINE_EVENTS)
            .map(event_from_row)
            .collect(),
    )
}

/// How a finished run counts as training data. Cancelled runs are censored:
/// they would have lasted at least as long as they did.
pub fn outcome(r: &RunRow) -> Option<Outcome> {
    match r.status {
        RunState::Completed => Some(Outcome::Completed),
        RunState::Failed => Some(Outcome::Failed),
        RunState::Cancelled => Some(Outcome::Censored),
        _ => None,
    }
}

pub fn command_stat(r: &CommandStatRow) -> CommandStat {
    CommandStat {
        repository_id: r.repository_id.clone(),
        command_key: r.command_key.clone(),
        samples: r.samples,
        log_mean: r.log_mean,
        log_m2: r.log_m2,
        failures: r.failures,
    }
}

/// Fits a fresh estimator from the database at `path`, on its own connection.
pub fn train(
    path: &std::path::Path,
    now: DateTime<Utc>,
) -> Result<runmarq_estimator::Estimator, StoreError> {
    let store = Store::open(path)?;
    let set = training_set(&store, crate::engine::HISTORY_LIMIT)?;
    Ok(runmarq_estimator::Estimator::fit(&set, now))
}

/// Loads up to `limit` finished runs with their timelines and signals.
pub fn training_set(store: &Store, limit: u32) -> Result<TrainingSet, StoreError> {
    let rows = store.finished_runs(limit)?;
    let rows: Vec<RunRow> = rows
        .into_iter()
        .filter(|r| outcome(r).is_some() && r.active_duration_ms.is_some() && r.ended_at.is_some())
        .collect();
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut events = store.events_for_runs(&ids)?;
    let mut signals = store.signals_for_runs(&ids)?;
    let files = store.repository_file_counts()?;
    let rows = stitch(rows, &mut events, &mut signals);
    let mut at_start = models_at_start(&rows);
    let runs = rows
        .into_iter()
        .filter_map(|r| {
            let model_at_start = at_start.remove(&r.id);
            let timeline = events
                .remove(&r.id)
                .map(|ev| timeline_from_rows(r.started_at, &ev));
            let snaps = signals
                .remove(&r.id)
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.active_ms as f64 / 1000.0, s.signals))
                .collect();
            Some(TrainingRun {
                outcome: outcome(&r)?,
                active_seconds: r.active_duration_ms? as f64 / 1000.0,
                ended_at: r.ended_at?,
                repo_files: r
                    .repository_id
                    .as_ref()
                    .and_then(|id| files.get(id).copied()),
                id: r.id,
                agent: r.agent,
                repository_id: r.repository_id,
                prompt: r.prompt_features,
                effort: r.effort,
                model: r.model,
                model_at_start,
                timeline,
                signals: snaps,
            })
        })
        .collect();
    let commands = store.command_stats()?.iter().map(command_stat).collect();
    Ok(TrainingSet { runs, commands })
}

/// For each run, the model of the latest earlier run in its session, which
/// is what the collector carries into a new run before its transcript is read.
fn models_at_start(rows: &[RunRow]) -> HashMap<String, String> {
    let mut order: Vec<&RunRow> = rows.iter().collect();
    order.sort_by(|a, b| {
        (a.session_id.as_str(), a.started_at).cmp(&(b.session_id.as_str(), b.started_at))
    });
    let mut out = HashMap::new();
    // (session, model) of the latest run seen with a model.
    let mut known: Option<(&str, &str)> = None;
    for r in order {
        match known {
            Some((session, model)) if session == r.session_id => {
                out.insert(r.id.clone(), model.to_string());
            }
            _ => known = None,
        }
        if let Some(m) = r.model.as_deref() {
            known = Some((r.session_id.as_str(), m));
        }
    }
    out
}

fn is_superseded(r: &RunRow) -> bool {
    r.exit_reason.as_deref() == Some(ExitReason::Superseded.as_str())
}

/// Repairs runs recorded before prompts that arrive mid-task continued the
/// run. A run cut off by a prompt while the agent was still busy is joined
/// with the run that prompt started; one that was not continued ends at its
/// last event, as does every cancelled run. Runs with neither a prompt nor a
/// tool call are dropped.
pub fn stitch(
    mut rows: Vec<RunRow>,
    events: &mut HashMap<String, Vec<EventRow>>,
    signals: &mut HashMap<String, Vec<SignalSnapshot>>,
) -> Vec<RunRow> {
    rows.sort_by(|a, b| {
        (a.session_id.as_str(), a.started_at).cmp(&(b.session_id.as_str(), b.started_at))
    });
    let last_event = |events: &HashMap<String, Vec<EventRow>>, id: &str| {
        events
            .get(id)
            .and_then(|ev| ev.iter().map(|e| e.source_timestamp).max())
    };
    let mut out: Vec<RunRow> = Vec::with_capacity(rows.len());
    let mut pending: Option<RunRow> = None;
    for mut r in rows {
        let ev = events.get(&r.id).map(Vec::as_slice).unwrap_or_default();
        let started_by_prompt = ev.iter().any(|e| {
            matches!(
                e.event_type,
                EventType::PromptSubmitted | EventType::RunStarted
            )
        });
        let tools = ev.iter().any(|e| {
            matches!(
                e.event_type,
                EventType::ToolStarted | EventType::ToolCompleted | EventType::ToolFailed
            )
        });
        if !started_by_prompt && !tools {
            continue;
        }
        if let Some(mut a) = pending.take() {
            let a_last = last_event(events, &a.id).unwrap_or(a.started_at);
            if a.session_id == r.session_id && r.started_at - a_last <= STITCH_WITHIN {
                let a_active = a.active_duration_ms.unwrap_or(0);
                let mut moved = events.remove(&r.id).unwrap_or_default();
                events.entry(a.id.clone()).or_default().append(&mut moved);
                let moved = signals.remove(&r.id).unwrap_or_default();
                signals
                    .entry(a.id.clone())
                    .or_default()
                    .extend(moved.into_iter().map(|mut s| {
                        s.active_ms += a_active;
                        s
                    }));
                a.status = r.status;
                a.exit_reason = r.exit_reason.take();
                a.ended_at = r.ended_at;
                a.active_duration_ms = Some(a_active + r.active_duration_ms.unwrap_or(0));
                a.waiting_duration_ms =
                    Some(a.waiting_duration_ms.unwrap_or(0) + r.waiting_duration_ms.unwrap_or(0));
                if a.prompt_features.is_none() {
                    a.prompt_features = r.prompt_features.take();
                    a.task_category = r.task_category;
                }
                a.effort = a.effort.or(r.effort.take());
                a.model = a.model.or(r.model.take());
                r = a;
            } else {
                out.push(end_at_last_event(a, a_last));
            }
        }
        if is_superseded(&r) {
            pending = Some(r);
        } else if r.status == RunState::Cancelled {
            let last = last_event(events, &r.id).unwrap_or(r.started_at);
            out.push(end_at_last_event(r, last));
        } else {
            out.push(r);
        }
    }
    if let Some(a) = pending {
        let a_last = last_event(events, &a.id).unwrap_or(a.started_at);
        out.push(end_at_last_event(a, a_last));
    }
    out.sort_by_key(|r| r.ended_at);
    out
}

/// A cut-off run is known to have lasted until its last event, no longer.
fn end_at_last_event(mut r: RunRow, last: DateTime<Utc>) -> RunRow {
    let span = (last - r.started_at).num_milliseconds().max(0) as u64;
    let active = span.saturating_sub(r.waiting_duration_ms.unwrap_or(0));
    r.active_duration_ms = Some(r.active_duration_ms.unwrap_or(active).min(active));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use runmarq_core::event::{AgentSource, TaskCategory};

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap() + Duration::seconds(secs)
    }

    fn run(id: &str, session: &str, start: i64, end: i64, status: RunState, exit: &str) -> RunRow {
        RunRow {
            id: id.into(),
            session_id: session.into(),
            source_turn_id: None,
            agent: AgentSource::ClaudeCode,
            repository_id: None,
            task_category: TaskCategory::BugFix,
            prompt_features: None,
            title: None,
            model: None,
            started_at: t(start),
            ended_at: Some(t(end)),
            last_event_at: t(end),
            wall_duration_ms: Some(((end - start) * 1000) as u64),
            active_duration_ms: Some(((end - start) * 1000) as u64),
            waiting_duration_ms: Some(0),
            status,
            exit_reason: Some(exit.into()),
            current_activity: None,
            current_tool: None,
            tool_calls: 0,
            tool_failures: 0,
            test_runs: 0,
            plan_total: None,
            plan_completed: None,
            initial_p10_ms: None,
            initial_p50_ms: None,
            initial_p90_ms: None,
            effort: None,
            permission_mode: None,
            signals: None,
        }
    }

    fn event(run: &str, kind: EventType, at: i64) -> EventRow {
        EventRow {
            id: format!("{run}_{at}_{}", kind.as_str()),
            run_id: Some(run.into()),
            session_id: None,
            event_type: kind,
            source_timestamp: t(at),
            received_timestamp: t(at),
            activity: None,
            tool: None,
            duration_ms: None,
            status: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn a_run_starts_with_its_sessions_model() {
        let mut a = run("a", "s", 0, 100, RunState::Completed, "agent_finished");
        a.model = Some("claude-opus-5-5".into());
        let b = run("b", "s", 200, 300, RunState::Completed, "agent_finished");
        let other = run("c", "t", 150, 400, RunState::Completed, "agent_finished");
        let m = models_at_start(&[b, other, a]);
        assert_eq!(m.get("b").map(String::as_str), Some("claude-opus-5-5"));
        assert!(
            !m.contains_key("a"),
            "the first run of a session starts unknown"
        );
        assert!(!m.contains_key("c"), "models stay in their session");
    }

    #[test]
    fn fragments_of_one_task_are_joined() {
        let rows = vec![
            run("a", "s", 0, 100, RunState::Cancelled, "superseded"),
            run("b", "s", 100, 250, RunState::Cancelled, "superseded"),
            run("c", "s", 250, 400, RunState::Completed, "agent_finished"),
        ];
        let mut events = HashMap::new();
        events.insert(
            "a".into(),
            vec![
                event("a", EventType::PromptSubmitted, 0),
                event("a", EventType::ToolCompleted, 100),
            ],
        );
        events.insert(
            "b".into(),
            vec![
                event("b", EventType::PromptSubmitted, 100),
                event("b", EventType::ToolCompleted, 248),
            ],
        );
        events.insert(
            "c".into(),
            vec![
                event("c", EventType::PromptSubmitted, 250),
                event("c", EventType::RunCompleted, 400),
            ],
        );
        let mut signals = HashMap::new();
        signals.insert(
            "c".into(),
            vec![SignalSnapshot {
                active_ms: 20_000,
                signals: Default::default(),
            }],
        );
        let out = stitch(rows, &mut events, &mut signals);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].status, RunState::Completed);
        assert_eq!(out[0].active_duration_ms, Some(400_000));
        assert_eq!(events["a"].len(), 6);
        assert_eq!(
            signals["a"][0].active_ms, 270_000,
            "snapshots move to the joined run's clock"
        );
    }

    #[test]
    fn interrupted_runs_end_at_their_last_event_and_phantoms_drop() {
        let rows = vec![
            run("a", "s", 0, 4000, RunState::Cancelled, "superseded"),
            run("b", "s", 4000, 4100, RunState::Completed, "agent_finished"),
            run("p", "s", 5000, 9000, RunState::Cancelled, "superseded"),
        ];
        let mut events = HashMap::new();
        events.insert(
            "a".into(),
            vec![
                event("a", EventType::PromptSubmitted, 0),
                event("a", EventType::ToolCompleted, 60),
            ],
        );
        events.insert(
            "b".into(),
            vec![
                event("b", EventType::PromptSubmitted, 4000),
                event("b", EventType::RunCompleted, 4100),
            ],
        );
        events.insert(
            "p".into(),
            vec![event("p", EventType::ContextCompacting, 5000)],
        );
        let out = stitch(rows, &mut events, &mut HashMap::new());
        assert_eq!(out.len(), 2, "a phantom with no prompt or tool is dropped");
        let a = out.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(
            a.active_duration_ms,
            Some(60_000),
            "a long silence is not run time"
        );
        assert_eq!(
            out.iter().find(|r| r.id == "b").unwrap().active_duration_ms,
            Some(100_000)
        );
    }
}
