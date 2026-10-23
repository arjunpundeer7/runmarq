//! Engine integration tests: fixture payloads flow through the real adapters
//! into the engine with controlled timestamps.

use chrono::{DateTime, Duration, TimeZone, Utc};
use runmarq_adapters::{for_source, NormalizeContext};
use runmarq_collector::engine::{Engine, STALE_AFTER};
use runmarq_core::api::{HistoryQuery, HookConfig, NotificationKind, Preferences, StreamMessage};
use runmarq_core::event::*;
use runmarq_core::forecast::ForecastStatus;
use runmarq_core::run::RunState;
use runmarq_storage::Store;
use std::path::Path;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap()
}

fn engine() -> Engine {
    Engine::new(Store::open_in_memory().unwrap()).unwrap()
}

fn lines(rel: &str) -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel);
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Feeds fixture lines, one every `step` seconds starting at `start`.
fn feed(
    e: &mut Engine,
    source: AgentSource,
    rel: &str,
    start: DateTime<Utc>,
    step: i64,
    titles: bool,
) -> Vec<StreamMessage> {
    let adapter = for_source(source);
    let mut msgs = Vec::new();
    for (i, line) in lines(rel).iter().enumerate() {
        let now = start + Duration::seconds(i as i64 * step);
        let ctx = NormalizeContext {
            now,
            hook_config: HookConfig {
                store_task_titles: titles,
                store_repository_names: true,
                prompt_sketches: true,
            },
            salt: b"test".to_vec(),
            launcher_id: None,
        };
        for env in adapter.normalize(&ctx, None, line.as_bytes()).events {
            msgs.extend(e.ingest(env, now).unwrap().messages);
        }
    }
    msgs
}

fn env(
    source: AgentSource,
    session: &str,
    turn: Option<&str>,
    t: EventType,
    at: DateTime<Utc>,
    payload: EventPayload,
) -> EventEnvelope {
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: format!(
            "ev_{}_{}_{}_{}",
            source.as_str(),
            session,
            t.as_str(),
            at.timestamp_millis()
        ),
        source,
        source_session_id: session.into(),
        source_turn_id: turn.map(String::from),
        timestamp: at,
        timestamp_origin: TimestampOrigin::HookInvocation,
        event_type: t,
        payload,
    }
}

/// Delivers an event as if it arrived when it happened.
fn ing(
    e: &mut Engine,
    ev: EventEnvelope,
) -> Result<runmarq_collector::engine::IngestOutcome, runmarq_collector::engine::EngineError> {
    let at = ev.timestamp;
    e.ingest(ev, at)
}

fn prompt(cat: TaskCategory) -> EventPayload {
    EventPayload {
        prompt: Some(PromptFeatures {
            category: cat,
            complexity: ComplexityBucket::Medium,
            complexity_score: 0.5,
            word_count: 10,
            list_items: 0,
            file_mentions: 0,
            mentions_tests: false,
            fingerprint: None,
            sketch: None,
        }),
        ..Default::default()
    }
}

fn tool(name: &str, id: &str, activity: ActivityCategory) -> EventPayload {
    EventPayload {
        tool_name: Some(name.into()),
        tool_use_id: Some(id.into()),
        activity: Some(activity),
        ..Default::default()
    }
}

fn all_runs(e: &mut Engine) -> Vec<runmarq_core::api::RunView> {
    let mut runs = e
        .history(
            &HistoryQuery {
                limit: Some(100),
                ..Default::default()
            },
            t0() + Duration::days(1),
        )
        .unwrap()
        .runs;
    runs.reverse();
    runs
}

fn notifications(msgs: &[StreamMessage]) -> Vec<NotificationKind> {
    msgs.iter()
        .filter_map(|m| match m {
            StreamMessage::Notification { notification } => Some(notification.kind),
            _ => None,
        })
        .collect()
}

#[test]
fn emitter_run_persists_correct_duration() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "sess",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "sess",
            None,
            EventType::ToolStarted,
            t0() + Duration::seconds(10),
            tool("Bash", "a", ActivityCategory::RunningTests),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "sess",
            None,
            EventType::ToolCompleted,
            t0() + Duration::seconds(40),
            tool("Bash", "a", ActivityCategory::RunningTests),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "sess",
            None,
            EventType::RunCompleted,
            t0() + Duration::seconds(95),
            EventPayload::default(),
        ),
    )
    .unwrap();
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 1);
    let r = &runs[0];
    assert_eq!(r.state, RunState::Completed);
    assert_eq!(r.wall_ms, 95_000);
    assert_eq!(r.active_ms, 95_000);
    assert_eq!(r.waiting_ms, 0);
    assert_eq!(r.tool_calls, 1);
    assert_eq!(r.exit_reason.as_deref(), Some("agent_finished"));
    assert!(r.initial_p50_seconds.is_some(), "initial forecast recorded");
    let detail = e.run_detail(&r.id, t0()).unwrap().unwrap();
    assert_eq!(detail.events.len(), 4);
    assert!(!detail.forecasts.is_empty());
}

#[test]
fn task_tools_build_the_plan_a_task_at_a_time() {
    let mut e = engine();
    let lines = lines("claude/task-tools.jsonl");
    let adapter = for_source(AgentSource::ClaudeCode);
    let mut plans = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let now = t0() + Duration::seconds(i as i64 * 5);
        let ctx = NormalizeContext {
            now,
            hook_config: HookConfig::default(),
            salt: b"test".to_vec(),
            launcher_id: None,
        };
        for env in adapter.normalize(&ctx, None, line.as_bytes()).events {
            let plan_event = env.event_type == EventType::PlanUpdated;
            e.ingest(env, now).unwrap();
            if plan_event {
                let r = all_runs(&mut e).pop().unwrap();
                let p = r.plan.unwrap();
                plans.push((p.completed_steps, p.total_steps));
            }
        }
    }
    assert_eq!(
        plans,
        vec![
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 3),
            (1, 3),
            (1, 2),
            (1, 2),
            (2, 2),
            (3, 3)
        ],
        "a task from an earlier run joins when it is updated"
    );
    assert_eq!(all_runs(&mut e).pop().unwrap().state, RunState::Completed);
}

#[test]
fn claude_fixture_full_lifecycle() {
    let mut e = engine();
    let msgs = feed(
        &mut e,
        AgentSource::ClaudeCode,
        "claude/basic-run.jsonl",
        t0(),
        10,
        false,
    );
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 1, "one prompt, one run");
    let r = &runs[0];
    assert_eq!(r.state, RunState::Completed);
    assert_eq!(r.task_category, TaskCategory::BugFix);
    // Prompt at line 2 (t=10s), Stop at line 17 (t=160s).
    assert_eq!(r.wall_ms, 150_000);
    // PermissionRequest at t=50s, first activity after it at t=70s.
    assert_eq!(r.waiting_ms, 20_000);
    assert_eq!(r.active_ms, 130_000);
    // Read, TodoWrite, Bash, Edit, Bash, TodoWrite.
    assert_eq!(r.tool_calls, 6);
    assert_eq!(r.tool_failures, 1);
    assert_eq!(r.plan.as_ref().unwrap().completed_steps, 3);
    assert_eq!(r.repository_name.as_deref(), Some("acme-web"));
    let kinds = notifications(&msgs);
    assert_eq!(
        kinds,
        vec![
            NotificationKind::AttentionRequired,
            NotificationKind::RunFinished
        ],
        "one per episode, deduplicated"
    );
    assert!(e.active_runs(t0()).is_empty());

    let d = e.run_detail(&r.id, t0()).unwrap().unwrap();
    assert!(d.phases.len() >= 3);
    assert!(d.forecasts.iter().all(|f| f.is_consistent()));
    assert!(
        d.forecasts
            .iter()
            .any(|f| f.status == ForecastStatus::Waiting),
        "waiting forecast recorded"
    );
}

#[test]
fn interrupts_are_cancelled_never_completed() {
    let mut e = engine();
    feed(
        &mut e,
        AgentSource::ClaudeCode,
        "claude/interrupted.jsonl",
        t0(),
        5,
        false,
    );
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].state, RunState::Cancelled);
    assert_eq!(runs[0].exit_reason.as_deref(), Some("user_interrupt"));
    assert_eq!(runs[1].state, RunState::Completed);
}

#[test]
fn new_prompt_supersedes_open_run() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            t0() + Duration::seconds(30),
            prompt(TaskCategory::SmallEdit),
        ),
    )
    .unwrap();
    let runs = all_runs(&mut e);
    assert_eq!(runs[0].state, RunState::Cancelled);
    assert_eq!(runs[0].exit_reason.as_deref(), Some("superseded"));
    assert_eq!(runs[1].state, RunState::Running);
}

#[test]
fn prompt_while_the_agent_is_busy_continues_the_run() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    let at = |secs: i64| t0() + Duration::seconds(secs);
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            at(0),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    let edit = || tool("Edit", "t1", ActivityCategory::EditingCode);
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolStarted, at(5), edit()),
    )
    .unwrap();
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolCompleted, at(20), edit()),
    )
    .unwrap();
    // A message typed during the turn arrives at the next tool boundary.
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            at(20),
            prompt(TaskCategory::Testing),
        ),
    )
    .unwrap();
    // A background command reports back while a long build is still running.
    let build = || tool("Bash", "t2", ActivityCategory::Building);
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolStarted, at(30), build()),
    )
    .unwrap();
    let injected = EventPayload {
        injected: Some(true),
        ..Default::default()
    };
    ing(
        &mut e,
        env(s, "x", None, EventType::PromptSubmitted, at(200), injected),
    )
    .unwrap();
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolCompleted, at(260), build()),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::RunCompleted,
            at(300),
            EventPayload::default(),
        ),
    )
    .unwrap();

    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 1, "one task, one run");
    assert_eq!(runs[0].state, RunState::Completed);
    assert_eq!(runs[0].active_ms, 300_000);
    assert_eq!(
        runs[0].task_category,
        TaskCategory::BugFix,
        "the first prompt names the task"
    );
}

#[test]
fn background_task_report_continues_after_a_pause() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    let at = |secs: i64| t0() + Duration::seconds(secs);
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            at(0),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    let read = || tool("Read", "t1", ActivityCategory::ReadingFiles);
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolStarted, at(2), read()),
    )
    .unwrap();
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolCompleted, at(3), read()),
    )
    .unwrap();
    let injected = EventPayload {
        injected: Some(true),
        ..Default::default()
    };
    ing(
        &mut e,
        env(s, "x", None, EventType::PromptSubmitted, at(120), injected),
    )
    .unwrap();
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 1);
    assert!(runs[0].state.is_working());
}

#[test]
fn superseded_run_ends_at_its_last_event() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    let at = |secs: i64| t0() + Duration::seconds(secs);
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            at(0),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    let read = || tool("Read", "t1", ActivityCategory::ReadingFiles);
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolStarted, at(30), read()),
    )
    .unwrap();
    ing(
        &mut e,
        env(s, "x", None, EventType::ToolCompleted, at(40), read()),
    )
    .unwrap();
    // Interrupted while thinking, and the next prompt comes ten minutes later.
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            at(640),
            prompt(TaskCategory::SmallEdit),
        ),
    )
    .unwrap();
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].exit_reason.as_deref(), Some("superseded"));
    assert_eq!(
        runs[0].active_ms, 40_000,
        "silence after the last event is not counted"
    );
}

#[test]
fn compaction_between_turns_does_not_start_a_run() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::ContextCompacting,
            t0(),
            EventPayload::default(),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::SubagentCompleted,
            t0() + Duration::seconds(60),
            EventPayload::default(),
        ),
    )
    .unwrap();
    assert!(all_runs(&mut e).is_empty());
}

#[test]
fn idle_without_stop_is_inferred_interrupt() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "x",
            None,
            EventType::AgentIdle,
            t0() + Duration::seconds(90),
            EventPayload::default(),
        ),
    )
    .unwrap();
    let r = &all_runs(&mut e)[0];
    assert_eq!(r.state, RunState::Cancelled);
    assert_eq!(r.exit_reason.as_deref(), Some("idle_without_completion"));
}

#[test]
fn cursor_and_codex_fixtures() {
    let mut e = engine();
    feed(
        &mut e,
        AgentSource::Cursor,
        "cursor/basic-run.jsonl",
        t0(),
        3,
        false,
    );
    feed(
        &mut e,
        AgentSource::Cursor,
        "cursor/aborted.jsonl",
        t0() + Duration::minutes(5),
        3,
        false,
    );
    feed(
        &mut e,
        AgentSource::Codex,
        "codex/hooks-run.jsonl",
        t0() + Duration::minutes(10),
        4,
        false,
    );
    let runs = all_runs(&mut e);
    let cursor: Vec<_> = runs
        .iter()
        .filter(|r| r.agent == AgentSource::Cursor)
        .collect();
    assert_eq!(cursor.len(), 2);
    assert_eq!(cursor[0].state, RunState::Completed);
    assert_eq!(
        cursor[0].tool_calls, 4,
        "completion-only adapter still counts tools"
    );
    assert_eq!(cursor[1].state, RunState::Cancelled);
    let codex: Vec<_> = runs
        .iter()
        .filter(|r| r.agent == AgentSource::Codex)
        .collect();
    assert_eq!(codex.len(), 1);
    assert_eq!(codex[0].state, RunState::Completed);
    assert_eq!(
        codex[0].waiting_ms, 4_000,
        "approval wait measured until next activity"
    );
    assert_eq!(codex[0].model.as_deref(), Some("gpt-5.1-codex"));
}

#[test]
fn concurrent_sessions_do_not_mix() {
    let mut e = engine();
    let c = AgentSource::ClaudeCode;
    let x = AgentSource::Codex;
    ing(
        &mut e,
        env(
            c,
            "same-id",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            x,
            "same-id",
            Some("t1"),
            EventType::PromptSubmitted,
            t0() + Duration::seconds(1),
            prompt(TaskCategory::Testing),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            c,
            "other",
            None,
            EventType::PromptSubmitted,
            t0() + Duration::seconds(2),
            prompt(TaskCategory::Research),
        ),
    )
    .unwrap();
    assert_eq!(e.active_runs(t0() + Duration::seconds(3)).len(), 3);
    ing(
        &mut e,
        env(
            x,
            "same-id",
            Some("t1"),
            EventType::RunCompleted,
            t0() + Duration::seconds(60),
            EventPayload::default(),
        ),
    )
    .unwrap();
    let active = e.active_runs(t0() + Duration::seconds(61));
    assert_eq!(active.len(), 2);
    assert!(
        active.iter().all(|r| r.agent == c),
        "a vendor session id shared across agents never collides"
    );
}

#[test]
fn duplicates_are_ignored() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    let ev = env(
        s,
        "d",
        None,
        EventType::PromptSubmitted,
        t0(),
        prompt(TaskCategory::BugFix),
    );
    assert!(!ing(&mut e, ev.clone()).unwrap().duplicate);
    assert!(ing(&mut e, ev.clone()).unwrap().duplicate);
    let tool_ev = env(
        s,
        "d",
        None,
        EventType::ToolStarted,
        t0() + Duration::seconds(1),
        tool("Read", "r1", ActivityCategory::ReadingFiles),
    );
    ing(&mut e, tool_ev.clone()).unwrap();
    ing(&mut e, tool_ev).unwrap();
    assert_eq!(all_runs(&mut e).len(), 1);
    assert_eq!(e.active_runs(t0())[0].tool_calls, 1);
    assert_eq!(e.stats.duplicates, 2);
}

#[test]
fn out_of_order_and_late_events() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    // Completion arrives before its start.
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::ToolCompleted,
            t0() + Duration::seconds(6),
            tool("Read", "r", ActivityCategory::ReadingFiles),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::ToolStarted,
            t0() + Duration::seconds(5),
            tool("Read", "r", ActivityCategory::ReadingFiles),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::RunCompleted,
            t0() + Duration::seconds(20),
            EventPayload::default(),
        ),
    )
    .unwrap();
    // Next run starts, then a late tool event from the first run arrives.
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::PromptSubmitted,
            t0() + Duration::seconds(60),
            prompt(TaskCategory::SmallEdit),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "o",
            None,
            EventType::ToolFailed,
            t0() + Duration::seconds(15),
            tool("Bash", "late", ActivityCategory::RunningTests),
        ),
    )
    .unwrap();
    let runs = all_runs(&mut e);
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs[0].state,
        RunState::Completed,
        "late event does not reopen a finished run"
    );
    assert_eq!(
        runs[1].tool_failures, 0,
        "late event does not pollute the new run"
    );
    assert_eq!(runs[1].state, RunState::Running);
}

#[test]
fn restart_recovery_marks_unknown_then_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("db.sqlite");
    let s = AgentSource::ClaudeCode;
    {
        let mut e = Engine::new(Store::open(&db).unwrap()).unwrap();
        ing(
            &mut e,
            env(
                s,
                "r",
                None,
                EventType::PromptSubmitted,
                t0(),
                prompt(TaskCategory::BugFix),
            ),
        )
        .unwrap();
    }
    let mut e = Engine::new(Store::open(&db).unwrap()).unwrap();
    e.recover(t0() + Duration::seconds(30)).unwrap();
    let active = e.active_runs(t0() + Duration::seconds(30));
    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0].state,
        RunState::Unknown,
        "after a restart the state is unknown, not completed"
    );
    assert_eq!(active[0].forecast, None);
    ing(
        &mut e,
        env(
            s,
            "r",
            None,
            EventType::ToolStarted,
            t0() + Duration::seconds(40),
            tool("Edit", "e", ActivityCategory::EditingCode),
        ),
    )
    .unwrap();
    assert_eq!(
        e.active_runs(t0() + Duration::seconds(41))[0].state,
        RunState::Running
    );
    ing(
        &mut e,
        env(
            s,
            "r",
            None,
            EventType::RunCompleted,
            t0() + Duration::seconds(80),
            EventPayload::default(),
        ),
    )
    .unwrap();
    assert_eq!(all_runs(&mut e)[0].state, RunState::Completed);
}

#[test]
fn silence_becomes_unknown_never_completed() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "q",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    assert!(e
        .tick(t0() + Duration::minutes(5))
        .iter()
        .all(|m| !matches!(m, StreamMessage::Notification { .. })));
    let msgs = e.tick(t0() + STALE_AFTER + Duration::minutes(1));
    assert_eq!(notifications(&msgs), vec![NotificationKind::ConnectionLost]);
    let r = &e.active_runs(t0() + STALE_AFTER + Duration::minutes(1))[0];
    assert_eq!(r.state, RunState::Unknown);
    assert_eq!(r.exit_reason.as_deref(), Some("stale"));
    // Repeated ticks do not repeat the notification.
    assert!(notifications(&e.tick(t0() + STALE_AFTER + Duration::minutes(2))).is_empty());
}

#[test]
fn a_run_ends_when_its_agent_process_exits() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ALIVE: AtomicBool = AtomicBool::new(true);
    let mut e = engine();
    e.set_process_check(|_| ALIVE.load(Ordering::SeqCst));
    let s = AgentSource::ClaudeCode;
    let agent = AgentProcess {
        pid: 4242,
        started: 7,
    };
    let mut p = prompt(TaskCategory::BugFix);
    p.agent_process = Some(agent);
    ing(
        &mut e,
        env(s, "k", None, EventType::PromptSubmitted, t0(), p),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "k",
            None,
            EventType::ToolStarted,
            t0() + Duration::seconds(20),
            tool("Bash", "b", ActivityCategory::RunningTests),
        ),
    )
    .unwrap();
    // A second session without a known process is left alone.
    ing(
        &mut e,
        env(
            s,
            "other",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();

    e.tick(t0() + Duration::seconds(60));
    assert_eq!(e.active_runs(t0() + Duration::seconds(60)).len(), 2);

    ALIVE.store(false, Ordering::SeqCst);
    e.tick(t0() + Duration::seconds(22));
    assert_eq!(
        e.active_runs(t0() + Duration::seconds(22)).len(),
        2,
        "events sent just before the exit get time to arrive"
    );
    let msgs = e.tick(t0() + Duration::seconds(65));
    assert!(
        notifications(&msgs).is_empty(),
        "closing the agent is not news"
    );
    let open = e.active_runs(t0() + Duration::seconds(65));
    assert_eq!(open.len(), 1);
    let r = all_runs(&mut e)
        .into_iter()
        .find(|r| r.state == RunState::Cancelled)
        .unwrap();
    assert_eq!(r.exit_reason.as_deref(), Some("process_exited"));
    assert_eq!(r.wall_ms, 65_000);
    assert_ne!(open[0].id, r.id);
}

#[test]
fn waiting_for_user_is_not_stale_and_pauses_forecast() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "w",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "w",
            None,
            EventType::ApprovalRequested,
            t0() + Duration::seconds(30),
            EventPayload::default(),
        ),
    )
    .unwrap();
    let late = t0() + Duration::hours(2);
    e.tick(late);
    let r = &e.active_runs(late)[0];
    assert_eq!(r.state, RunState::WaitingForUser);
    assert_eq!(r.forecast.as_ref().unwrap().status, ForecastStatus::Waiting);
    assert_eq!(
        r.active_ms, 30_000,
        "waiting time is excluded from active time"
    );
}

#[test]
fn session_end_and_launcher_exit_close_open_runs() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "se",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "se",
            None,
            EventType::SessionEnded,
            t0() + Duration::seconds(10),
            EventPayload::default(),
        ),
    )
    .unwrap();
    assert_eq!(
        all_runs(&mut e)[0].exit_reason.as_deref(),
        Some("session_ended")
    );

    let mut p = prompt(TaskCategory::Testing);
    p.launcher_id = Some("launch-1".into());
    ing(
        &mut e,
        env(
            AgentSource::Codex,
            "cx",
            Some("t1"),
            EventType::PromptSubmitted,
            t0() + Duration::seconds(20),
            p,
        ),
    )
    .unwrap();
    e.launcher_exited("launch-1", t0() + Duration::seconds(50))
        .unwrap();
    let r = all_runs(&mut e)
        .into_iter()
        .find(|r| r.agent == AgentSource::Codex)
        .unwrap();
    assert_eq!(r.state, RunState::Cancelled);
    assert_eq!(r.exit_reason.as_deref(), Some("process_exited"));
}

#[test]
fn forecast_reacts_to_repeated_test_failures() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "f",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    let p50 = |e: &mut Engine, at| {
        e.active_runs(at)[0]
            .forecast
            .as_ref()
            .unwrap()
            .remaining_p50_seconds
            .unwrap()
    };
    let base = p50(&mut e, t0() + Duration::seconds(1));
    for i in 0..3 {
        let at = t0() + Duration::seconds(10 + i * 10);
        ing(
            &mut e,
            env(
                s,
                "f",
                None,
                EventType::ToolFailed,
                at,
                tool("Bash", &format!("t{i}"), ActivityCategory::RunningTests),
            ),
        )
        .unwrap();
    }
    let after = p50(&mut e, t0() + Duration::seconds(41));
    assert!(after > base, "estimate increased: {base} -> {after}");
    let id = e.active_runs(t0())[0].id.clone();
    let d = e.run_detail(&id, t0()).unwrap().unwrap();
    assert!(
        d.changes.iter().any(|c| c.message.contains("test failure")),
        "{:?}",
        d.changes
    );
}

#[test]
fn history_learning_changes_estimates() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    // 30 finished bug fixes in this setup take ~20 minutes each.
    for i in 0..30 {
        let start = t0() + Duration::hours(i);
        let sess = format!("h{i}");
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::PromptSubmitted,
                start,
                prompt(TaskCategory::BugFix),
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::RunCompleted,
                start + Duration::minutes(20),
                EventPayload::default(),
            ),
        )
        .unwrap();
    }
    let start = t0() + Duration::hours(40);
    ing(
        &mut e,
        env(
            s,
            "new",
            None,
            EventType::PromptSubmitted,
            start,
            prompt(TaskCategory::BugFix),
        ),
    )
    .unwrap();
    let f = e.active_runs(start)[0].forecast.clone().unwrap();
    let p50 = f.remaining_p50_seconds.unwrap();
    assert!((900..1500).contains(&p50), "learned ~20 min, got {p50}s");
    assert!(f.reason_codes.iter().any(|r| r.starts_with("history_")));
}

#[test]
fn titles_and_repo_names_respect_preferences() {
    let mut e = engine();
    feed(
        &mut e,
        AgentSource::ClaudeCode,
        "claude/basic-run.jsonl",
        t0(),
        1,
        true,
    );
    assert_eq!(
        all_runs(&mut e)[0].title,
        None,
        "titles stay off until the user opts in, even if a hook sends one"
    );

    let mut e = engine();
    e.set_preferences(Preferences {
        store_task_titles: true,
        ..Default::default()
    })
    .unwrap();
    feed(
        &mut e,
        AgentSource::ClaudeCode,
        "claude/basic-run.jsonl",
        t0(),
        1,
        true,
    );
    assert!(all_runs(&mut e)[0].title.is_some());
    e.set_preferences(Preferences {
        store_task_titles: false,
        store_repository_names: false,
        ..Default::default()
    })
    .unwrap();
    let r = &all_runs(&mut e)[0];
    assert_eq!(r.title, None, "disabling titles erases stored titles");
    assert_eq!(r.repository_name, None);
}

#[test]
fn quiet_mode_suppresses_notifications() {
    let mut e = engine();
    e.set_preferences(Preferences {
        quiet_mode: true,
        ..Default::default()
    })
    .unwrap();
    let msgs = feed(
        &mut e,
        AgentSource::ClaudeCode,
        "claude/basic-run.jsonl",
        t0(),
        10,
        false,
    );
    assert!(notifications(&msgs).is_empty());
}

#[test]
fn short_runs_do_not_notify() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(
            s,
            "short",
            None,
            EventType::PromptSubmitted,
            t0(),
            prompt(TaskCategory::SmallEdit),
        ),
    )
    .unwrap();
    let o = ing(
        &mut e,
        env(
            s,
            "short",
            None,
            EventType::RunCompleted,
            t0() + Duration::seconds(5),
            EventPayload::default(),
        ),
    )
    .unwrap();
    assert!(notifications(&o.messages).is_empty());
}

#[test]
fn invalid_events_are_rejected() {
    let mut e = engine();
    let mut bad = env(
        AgentSource::ClaudeCode,
        "bad id with spaces",
        None,
        EventType::Heartbeat,
        t0(),
        EventPayload::default(),
    );
    assert!(ing(&mut e, bad.clone()).is_err());
    bad.source_session_id = "ok".into();
    bad.schema_version = 7;
    assert!(ing(&mut e, bad).is_err());
    assert_eq!(e.stats.rejected, 2);
}

fn transcript_line(id: &str, at: DateTime<Utc>, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"id":"{id}","model":"claude-x","usage":{{"input_tokens":1000,"output_tokens":{output}}}}}}}"#,
        at.to_rfc3339()
    )
}

#[test]
fn transcripts_and_effort_become_signals_without_storing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let start = t0();
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            transcript_line("old", start - Duration::hours(1), 999),
            transcript_line("m1", start + Duration::seconds(2), 120),
            transcript_line("m2", start + Duration::seconds(5), 80)
        ),
    )
    .unwrap();
    let mut e = engine();
    let mut p = prompt(TaskCategory::BugFix);
    p.effort = Some("high".into());
    p.permission_mode = Some("default".into());
    p.transcript_path = Some(path.to_string_lossy().into());
    p.cwd = Some(dir.path().to_string_lossy().into());
    let s = AgentSource::ClaudeCode;
    ing(
        &mut e,
        env(s, "sig", None, EventType::PromptSubmitted, start, p),
    )
    .unwrap();
    let now = start + Duration::seconds(6);
    assert!(!e.run_probes(now).is_empty());
    let run = e.active_runs(now).remove(0);
    assert_eq!(run.effort.as_deref(), Some("high"));
    let sig = run.signals.clone().unwrap();
    assert_eq!(
        (sig.turns, sig.output_tokens, sig.context_tokens),
        (2, 200, 1000)
    );

    for ev in e.store().events_for_run(&run.id).unwrap() {
        let m = ev.metadata.to_string();
        assert!(
            !m.contains("session.jsonl") && !m.contains("transcriptPath"),
            "{m}"
        );
        assert!(!m.contains("cwd") && !m.contains("effort"), "{m}");
    }
    let stored = e.store().get_run(&run.id).unwrap().unwrap();
    assert_eq!(stored.signals.unwrap().output_tokens, 200);
    assert_eq!(stored.permission_mode.as_deref(), Some("default"));

    let mut prefs = e.preferences().clone();
    prefs.read_transcripts = false;
    prefs.sample_git_diff = false;
    e.set_preferences(prefs).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    writeln!(
        f,
        "{}",
        transcript_line("m3", start + Duration::seconds(8), 50)
    )
    .unwrap();
    assert!(e.run_probes(start + Duration::seconds(9)).is_empty());
    let run = e.active_runs(start + Duration::seconds(9)).remove(0);
    assert_eq!(
        run.signals.unwrap().output_tokens,
        200,
        "no reads once turned off"
    );
}

#[test]
fn only_json_lines_transcripts_are_opened() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("notes.txt");
    std::fs::write(
        &other,
        transcript_line("m1", t0() + Duration::seconds(1), 10) + "\n",
    )
    .unwrap();
    let mut e = engine();
    let mut p = prompt(TaskCategory::BugFix);
    p.transcript_path = Some(other.to_string_lossy().into());
    ing(
        &mut e,
        env(
            AgentSource::ClaudeCode,
            "txt",
            None,
            EventType::PromptSubmitted,
            t0(),
            p,
        ),
    )
    .unwrap();
    assert!(e.run_probes(t0() + Duration::seconds(2)).is_empty());
}

#[test]
fn command_durations_are_learned_per_repository() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    for i in 0..3i64 {
        let start = t0() + Duration::hours(i);
        let sess = format!("cmd{i}");
        let mut p = prompt(TaskCategory::Testing);
        p.repository = Some(RepositoryRef {
            id: "repo_a".into(),
            display_name: None,
        });
        ing(
            &mut e,
            env(s, &sess, None, EventType::PromptSubmitted, start, p),
        )
        .unwrap();
        let mut run_tests = tool("Bash", "t", ActivityCategory::RunningTests);
        run_tests.command_key = Some("cmd_0123456789abcdef0123".into());
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolStarted,
                start + Duration::seconds(5),
                run_tests.clone(),
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolCompleted,
                start + Duration::seconds(65),
                run_tests,
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::RunCompleted,
                start + Duration::seconds(70),
                EventPayload::default(),
            ),
        )
        .unwrap();
    }
    let stats = e.store().command_stats().unwrap();
    assert_eq!(stats.len(), 1);
    let c = &stats[0];
    assert_eq!(
        (c.repository_id.as_str(), c.samples, c.failures),
        ("repo_a", 3, 0)
    );
    assert!(
        (c.log_mean.exp() - 60.0).abs() < 0.5,
        "{}",
        c.log_mean.exp()
    );
}

#[test]
fn timelines_feed_training_and_model_status() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    for i in 0..12i64 {
        let start = t0() + Duration::hours(i);
        let sess = format!("tl{i}");
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::PromptSubmitted,
                start,
                prompt(TaskCategory::BugFix),
            ),
        )
        .unwrap();
        let read = tool("Read", "r", ActivityCategory::ReadingFiles);
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolStarted,
                start + Duration::seconds(5),
                read.clone(),
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolCompleted,
                start + Duration::seconds(6),
                read,
            ),
        )
        .unwrap();
        let test = tool("Bash", "t", ActivityCategory::RunningTests);
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolStarted,
                start + Duration::seconds(20),
                test.clone(),
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::ToolCompleted,
                start + Duration::seconds(50),
                test,
            ),
        )
        .unwrap();
        ing(
            &mut e,
            env(
                s,
                &sess,
                None,
                EventType::RunCompleted,
                start + Duration::seconds(60),
                EventPayload::default(),
            ),
        )
        .unwrap();
    }
    let set = runmarq_collector::learning::training_set(e.store(), 100).unwrap();
    assert_eq!(set.runs.len(), 12);
    let tl = set.runs[0].timeline.as_ref().unwrap();
    assert!(tl.len() >= 5, "prompt, tools and completion: {}", tl.len());
    let snap = set.runs[0].snapshot_at(55.0);
    assert_eq!((snap.tool_calls, snap.test_runs), (2, 1));
    assert!(snap.last_test_passed);

    let insights = e.insights().unwrap();
    assert_eq!(insights.model.finished_runs, 12);
    let phase = insights
        .model
        .components
        .iter()
        .find(|c| c.key == "phase")
        .unwrap();
    assert!(phase.active, "{phase:?}");
    assert!(!insights.model.calibration.active);
}

#[test]
fn turning_off_similar_prompts_erases_sketches() {
    let mut e = engine();
    let s = AgentSource::ClaudeCode;
    let mut p = prompt(TaskCategory::BugFix);
    if let Some(f) = p.prompt.as_mut() {
        f.sketch = Some(vec![3; 256]);
    }
    ing(
        &mut e,
        env(s, "sk", None, EventType::PromptSubmitted, t0(), p.clone()),
    )
    .unwrap();
    ing(
        &mut e,
        env(
            s,
            "sk",
            None,
            EventType::RunCompleted,
            t0() + Duration::seconds(30),
            EventPayload::default(),
        ),
    )
    .unwrap();
    let id = all_runs(&mut e)[0].id.clone();
    assert!(e
        .store()
        .get_run(&id)
        .unwrap()
        .unwrap()
        .prompt_features
        .unwrap()
        .sketch
        .is_some());
    for ev in e.store().events_for_run(&id).unwrap() {
        assert!(
            ev.metadata.get("prompt").is_none(),
            "prompt features live on the run only"
        );
    }

    let mut prefs = e.preferences().clone();
    prefs.similar_prompts = false;
    e.set_preferences(prefs).unwrap();
    assert!(e
        .store()
        .get_run(&id)
        .unwrap()
        .unwrap()
        .prompt_features
        .unwrap()
        .sketch
        .is_none());

    ing(
        &mut e,
        env(
            s,
            "sk2",
            None,
            EventType::PromptSubmitted,
            t0() + Duration::minutes(5),
            p,
        ),
    )
    .unwrap();
    let open = e.active_runs(t0() + Duration::minutes(5)).remove(0);
    let row = e.store().get_run(&open.id).unwrap().unwrap();
    assert!(
        row.prompt_features.unwrap().sketch.is_none(),
        "new sketches are dropped while off"
    );
}
