//! Generates the website's interactive demonstration from the real engine.
//!
//! A scripted, deterministic Claude Code run is replayed through runmarq's
//! actual state machine and estimator, on top of 40 sample past runs, and the
//! run view after each event is written as JSON. The website plays these
//! frames back; nothing in the demo's numbers is hand-written.
//!
//!     cargo run -p runmarq-collector --example demo_run > apps/web/content/demo-run.json

use chrono::{DateTime, Duration, TimeZone, Utc};
use runmarq_collector::Engine;
use runmarq_core::api::RunView;
use runmarq_core::event::*;
use runmarq_storage::Store;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame {
    t: i64,
    label: &'static str,
    detail: &'static str,
    run: RunView,
    change: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Demo {
    sample: bool,
    description: &'static str,
    model_version: &'static str,
    history_runs: u32,
    frames: Vec<Frame>,
}

fn env(
    t: EventType,
    at: DateTime<Utc>,
    session: &str,
    n: u32,
    payload: EventPayload,
) -> EventEnvelope {
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: format!("ev_demo_{session}_{n}"),
        source: AgentSource::ClaudeCode,
        source_session_id: session.into(),
        source_turn_id: None,
        timestamp: at,
        timestamp_origin: TimestampOrigin::HookInvocation,
        event_type: t,
        payload,
    }
}

fn prompt() -> EventPayload {
    EventPayload {
        repository: Some(RepositoryRef {
            id: "repo_demo".into(),
            display_name: Some("acme-web".into()),
        }),
        prompt: Some(PromptFeatures {
            category: TaskCategory::FeatureImplementation,
            complexity: ComplexityBucket::Medium,
            complexity_score: 0.5,
            word_count: 18,
            list_items: 0,
            file_mentions: 1,
            mentions_tests: true,
            fingerprint: None,
            sketch: None,
        }),
        title: Some("Add password reset to the login flow".into()),
        ..Default::default()
    }
}

const TEST_COMMAND: &str = "cmd_00demo00tests00000";

fn tool(name: &str, id: &str, a: ActivityCategory) -> EventPayload {
    EventPayload {
        tool_name: Some(name.into()),
        tool_use_id: Some(id.into()),
        activity: Some(a),
        command_key: (a == ActivityCategory::RunningTests).then(|| TEST_COMMAND.to_string()),
        ..Default::default()
    }
}

/// One sample past run: read, edit, test, and for some runs a failed test,
/// another edit and a passing test, ending after `total` seconds.
fn past_run(total: i64, retry: bool) -> Vec<(i64, EventType, EventPayload)> {
    use ActivityCategory::*;
    use EventType::*;
    let at = |f: f64| (total as f64 * f) as i64;
    let mut v = vec![
        (at(0.05), ToolStarted, tool("Read", "r", ReadingFiles)),
        (at(0.05) + 1, ToolCompleted, tool("Read", "r", ReadingFiles)),
        (at(0.20), ToolStarted, tool("Edit", "e1", EditingCode)),
        (at(0.20) + 2, ToolCompleted, tool("Edit", "e1", EditingCode)),
    ];
    if retry {
        v.push((at(0.35), ToolStarted, tool("Bash", "t1", RunningTests)));
        v.push((at(0.35) + 42, ToolFailed, tool("Bash", "t1", RunningTests)));
        v.push((at(0.55), ToolStarted, tool("Edit", "e2", EditingCode)));
        v.push((at(0.55) + 2, ToolCompleted, tool("Edit", "e2", EditingCode)));
        v.push((at(0.70), ToolStarted, tool("Bash", "t2", RunningTests)));
        v.push((
            at(0.70) + 44,
            ToolCompleted,
            tool("Bash", "t2", RunningTests),
        ));
    } else {
        v.push((at(0.55), ToolStarted, tool("Bash", "t1", RunningTests)));
        v.push((
            at(0.55) + 43,
            ToolCompleted,
            tool("Bash", "t1", RunningTests),
        ));
    }
    v.push((total, RunCompleted, EventPayload::default()));
    v
}

const HISTORY_RUNS: u32 = 40;

fn main() {
    let mut e = Engine::new(Store::open_in_memory().unwrap()).unwrap();
    e.set_preferences(runmarq_core::api::Preferences {
        store_task_titles: true,
        ..Default::default()
    })
    .unwrap();

    // 40 sample past feature runs in this repository: median about 7 minutes,
    // a third of them with a failed test and a retry.
    let base = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let minutes = [
        5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 9.0, 6.2, 7.8, 10.5, 5.0, 6.8, 7.2, 8.4, 6.6,
    ];
    for i in 0..HISTORY_RUNS {
        let start = base + Duration::hours(i as i64 * 7);
        let s = format!("hist{i}");
        let mut p = prompt();
        p.title = None;
        e.ingest(env(EventType::PromptSubmitted, start, &s, 0, p), start)
            .unwrap();
        let total = (minutes[i as usize % minutes.len()] * 60.0) as i64;
        let retry = i % 3 == 0;
        let total = if retry { total + 90 } else { total };
        for (n, (t, et, payload)) in past_run(total, retry).into_iter().enumerate() {
            let at = start + Duration::seconds(t);
            e.ingest(env(et, at, &s, 1 + n as u32, payload), at)
                .unwrap();
        }
    }

    let t0 = Utc.with_ymd_and_hms(2026, 10, 1, 14, 0, 0).unwrap();
    let script: Vec<(i64, &str, &str, EventType, EventPayload)> = vec![
        (0, "Prompt submitted", "The agent accepts the prompt. runmarq classifies it locally and makes a first estimate.", EventType::PromptSubmitted, prompt()),
        (18, "Reading files", "The agent reads the login controller and its tests.", EventType::ToolStarted, tool("Read", "t1", ActivityCategory::ReadingFiles)),
        (74, "Editing code", "Changes go into three files.", EventType::ToolStarted, tool("Edit", "t2", ActivityCategory::EditingCode)),
        (96, "Edit finished", "", EventType::ToolCompleted, tool("Edit", "t2", ActivityCategory::EditingCode)),
        (150, "Running tests", "The agent runs the test suite.", EventType::ToolStarted, tool("Bash", "t3", ActivityCategory::RunningTests)),
        (196, "Tests failed", "One test fails. The estimate widens and moves out.", EventType::ToolFailed, tool("Bash", "t3", ActivityCategory::RunningTests)),
        (238, "Running tests again", "A second attempt after a fix.", EventType::ToolStarted, tool("Bash", "t4", ActivityCategory::RunningTests)),
        (281, "Tests failed again", "Repeated failures push the estimate further out.", EventType::ToolFailed, tool("Bash", "t4", ActivityCategory::RunningTests)),
        (300, "Approval requested", "The agent asks permission to run a migration. The estimate pauses.", EventType::ApprovalRequested, EventPayload { tool_name: Some("Bash".into()), ..Default::default() }),
        (342, "Editing code", "You approve; work resumes.", EventType::ToolStarted, tool("Edit", "t5", ActivityCategory::EditingCode)),
        (365, "Edit finished", "", EventType::ToolCompleted, tool("Edit", "t5", ActivityCategory::EditingCode)),
        (380, "Running tests", "", EventType::ToolStarted, tool("Bash", "t6", ActivityCategory::RunningTests)),
        (418, "Tests passed", "The suite passes with no edits since. The estimate tightens.", EventType::ToolCompleted, tool("Bash", "t6", ActivityCategory::RunningTests)),
        (452, "Run finished", "The agent reports the end of its turn. runmarq records the actual duration.", EventType::RunCompleted, EventPayload::default()),
    ];

    let mut frames = Vec::new();
    for (i, (t, label, detail, et, p)) in script.into_iter().enumerate() {
        let at = t0 + Duration::seconds(t);
        e.ingest(env(et, at, "demo", 100 + i as u32, p), at)
            .unwrap();
        let active = e.active_runs(at);
        let run = match active.into_iter().next() {
            Some(r) => r,
            None => e
                .history(&Default::default(), at)
                .unwrap()
                .runs
                .into_iter()
                .next()
                .unwrap(),
        };
        let change = e.run_detail(&run.id, at).unwrap().and_then(|d| {
            d.changes
                .last()
                .filter(|c| c.at == at)
                .map(|c| c.message.clone())
        });
        frames.push(Frame {
            t,
            label,
            detail,
            run,
            change,
        });
    }

    let demo = Demo {
        sample: true,
        description: "A scripted Claude Code run replayed through runmarq's state engine and estimator, with 40 sample past runs as history.",
        model_version: runmarq_estimator::MODEL_VERSION,
        history_runs: HISTORY_RUNS,
        frames,
    };
    println!("{}", serde_json::to_string_pretty(&demo).unwrap());
}
