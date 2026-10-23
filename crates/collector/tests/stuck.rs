//! Stuck runs through the engine: flagged with a reason, notified once per
//! episode, and cleared when progress resumes.

use chrono::{DateTime, Duration, TimeZone, Utc};
use runmarq_collector::engine::Engine;
use runmarq_core::api::{AttentionKind, NotificationKind, RunView, StreamMessage};
use runmarq_core::event::*;
use runmarq_storage::Store;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap()
}

fn at(secs: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(secs)
}

fn engine() -> Engine {
    Engine::new(Store::open_in_memory().unwrap()).unwrap()
}

fn send(e: &mut Engine, kind: EventType, secs: i64, payload: EventPayload) -> Vec<StreamMessage> {
    let ts = at(secs);
    let env = EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: format!(
            "ev_{}_{}_{secs}",
            kind.as_str(),
            payload.tool_use_id.clone().unwrap_or_default()
        ),
        source: AgentSource::ClaudeCode,
        source_session_id: "stuck".into(),
        source_turn_id: None,
        timestamp: ts,
        timestamp_origin: TimestampOrigin::HookInvocation,
        event_type: kind,
        payload,
    };
    e.ingest(env, ts).unwrap().messages
}

fn start(e: &mut Engine) {
    let payload = EventPayload {
        prompt: Some(PromptFeatures {
            category: TaskCategory::BugFix,
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
    };
    send(e, EventType::PromptSubmitted, 0, payload);
}

fn bash(id: &str, command: &str) -> EventPayload {
    EventPayload {
        tool_name: Some("Bash".into()),
        tool_use_id: Some(id.into()),
        activity: Some(ActivityCategory::RunningTests),
        command_key: Some(command.into()),
        ..Default::default()
    }
}

fn stuck_notifications(msgs: &[StreamMessage]) -> Vec<(String, String)> {
    msgs.iter()
        .filter_map(|m| match m {
            StreamMessage::Notification { notification }
                if notification.kind == NotificationKind::Stuck =>
            {
                Some((notification.title.clone(), notification.body.clone()))
            }
            _ => None,
        })
        .collect()
}

fn run(e: &mut Engine, secs: i64) -> RunView {
    e.active_runs(at(secs)).remove(0)
}

/// Starts and fails the same test command `n` times, beginning at `from`.
fn fail_tests(e: &mut Engine, from: i64, n: i64) -> Vec<StreamMessage> {
    let mut msgs = Vec::new();
    for i in 0..n {
        let id = format!("t{from}_{i}");
        let s = from + i * 20;
        msgs.extend(send(e, EventType::ToolStarted, s, bash(&id, "cmd_tests")));
        msgs.extend(send(
            e,
            EventType::ToolFailed,
            s + 10,
            bash(&id, "cmd_tests"),
        ));
    }
    msgs
}

#[test]
fn repeated_failures_notify_once_per_episode() {
    let mut e = engine();
    start(&mut e);
    let msgs = fail_tests(&mut e, 10, 2);
    assert!(stuck_notifications(&msgs).is_empty());
    assert_eq!(
        run(&mut e, 60).attention,
        None,
        "two failures are not a pattern"
    );

    let msgs = fail_tests(&mut e, 60, 1);
    assert_eq!(
        stuck_notifications(&msgs),
        vec![(
            "Claude Code may be stuck".to_string(),
            "The same command failed 3 times in a row".to_string()
        )]
    );
    let r = run(&mut e, 80);
    assert_eq!(r.attention, Some(AttentionKind::Stuck));
    assert_eq!(
        r.attention_detail.as_deref(),
        Some("The same command failed 3 times in a row")
    );

    // Still failing: the reason updates, the notification does not repeat.
    let mut msgs = fail_tests(&mut e, 100, 1);
    msgs.extend(e.tick(at(130)));
    assert!(stuck_notifications(&msgs).is_empty());
    assert_eq!(
        run(&mut e, 130).attention_detail.as_deref(),
        Some("The same command failed 4 times in a row")
    );

    // Progress clears the episode at once.
    send(&mut e, EventType::ToolStarted, 140, bash("ok", "cmd_tests"));
    send(
        &mut e,
        EventType::ToolCompleted,
        150,
        bash("ok", "cmd_tests"),
    );
    let r = run(&mut e, 151);
    assert_eq!((r.attention, r.attention_detail), (None, None));

    // A new episode is a new notification.
    let msgs = fail_tests(&mut e, 160, 3);
    assert_eq!(stuck_notifications(&msgs).len(), 1);
}

#[test]
fn a_tool_call_running_far_too_long_is_flagged_on_tick() {
    let mut e = engine();
    start(&mut e);
    send(
        &mut e,
        EventType::ToolStarted,
        10,
        bash("long", "cmd_build"),
    );
    assert!(stuck_notifications(&e.tick(at(5 * 60))).is_empty());
    assert_eq!(run(&mut e, 5 * 60).attention, None);

    let msgs = e.tick(at(10 + 11 * 60));
    assert_eq!(
        stuck_notifications(&msgs),
        vec![(
            "Claude Code may be stuck".to_string(),
            "Bash has been running for 11 min".to_string()
        )]
    );
    assert!(
        msgs.iter().any(|m| matches!(m, StreamMessage::RunUpdated { run } if run.attention == Some(AttentionKind::Stuck))),
        "the run card hears about it"
    );
    assert!(stuck_notifications(&e.tick(at(10 + 12 * 60))).is_empty());

    send(
        &mut e,
        EventType::ToolCompleted,
        10 + 13 * 60,
        bash("long", "cmd_build"),
    );
    assert_eq!(run(&mut e, 10 + 13 * 60).attention, None);
}

#[test]
fn the_preference_silences_the_notification_but_not_the_flag() {
    let mut e = engine();
    let mut prefs = e.preferences().clone();
    assert!(prefs.notify_on_stuck, "on by default");
    prefs.notify_on_stuck = false;
    e.set_preferences(prefs).unwrap();
    start(&mut e);
    let msgs = fail_tests(&mut e, 10, 3);
    assert!(stuck_notifications(&msgs).is_empty());
    assert_eq!(run(&mut e, 80).attention, Some(AttentionKind::Stuck));
}

#[test]
fn waiting_for_approval_is_never_stuck() {
    let mut e = engine();
    start(&mut e);
    send(&mut e, EventType::ToolStarted, 10, bash("w", "cmd_x"));
    send(
        &mut e,
        EventType::ApprovalRequested,
        11,
        EventPayload::default(),
    );
    let msgs = e.tick(at(15 * 60));
    assert!(stuck_notifications(&msgs).is_empty());
    assert_eq!(
        run(&mut e, 15 * 60).attention,
        Some(AttentionKind::ApprovalRequired)
    );
}
