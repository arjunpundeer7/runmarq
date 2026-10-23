//! Contract tests: replay fixture payloads through each adapter and check
//! the normalized output, then round-trip install / verify / uninstall
//! against a temporary home directory.

use chrono::Utc;
use runmarq_adapters::{codex::ExecStream, for_source, InstallOptions, NormalizeContext, Scope};
use runmarq_core::api::HookConfig;
use runmarq_core::event::{ActivityCategory, AgentSource, EventEnvelope, EventType, TaskCategory};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect()
}

fn ctx(titles: bool) -> NormalizeContext {
    NormalizeContext {
        now: Utc::now(),
        hook_config: HookConfig {
            store_task_titles: titles,
            store_repository_names: true,
            prompt_sketches: true,
        },
        salt: b"fixture-salt".to_vec(),
        launcher_id: None,
    }
}

fn replay(
    source: AgentSource,
    rel: &str,
    titles: bool,
) -> (Vec<EventEnvelope>, Vec<Option<&'static str>>) {
    let adapter = for_source(source);
    let c = ctx(titles);
    let mut events = Vec::new();
    let mut outs = Vec::new();
    for line in fixture(rel) {
        let n = adapter.normalize(&c, None, line.as_bytes());
        for e in &n.events {
            e.validate(Utc::now())
                .unwrap_or_else(|err| panic!("{rel}: invalid event {err}: {e:?}"));
            let json = serde_json::to_string(e).unwrap();
            for secret in [
                "npm test -- login",
                "Run login tests",
                "export function login",
                "The login test passes now",
                "Found it.",
            ] {
                assert!(
                    !json.contains(secret),
                    "{rel}: leaked '{secret}' into {json}"
                );
            }
        }
        events.extend(n.events);
        outs.push(n.stdout);
    }
    (events, outs)
}

fn types(events: &[EventEnvelope]) -> Vec<EventType> {
    events.iter().map(|e| e.event_type).collect()
}

#[test]
fn claude_basic_run() {
    let (events, outs) = replay(AgentSource::ClaudeCode, "claude/basic-run.jsonl", false);
    use EventType::*;
    assert_eq!(
        types(&events),
        vec![
            SessionStarted,
            PromptSubmitted,
            ToolStarted,
            ToolCompleted,
            ToolCompleted,
            PlanUpdated,
            ApprovalRequested,
            ApprovalRequested,
            ToolStarted,
            ToolFailed,
            ToolStarted,
            ToolCompleted,
            SubagentStarted,
            SubagentCompleted,
            ToolStarted,
            ToolCompleted,
            ToolCompleted,
            PlanUpdated,
            RunCompleted,
            AgentIdle,
            SessionEnded
        ]
    );
    assert!(
        outs.iter().all(Option::is_none),
        "Claude hooks must print nothing"
    );
    let prompt = events
        .iter()
        .find(|e| e.event_type == PromptSubmitted)
        .unwrap();
    let f = prompt.payload.prompt.as_ref().unwrap();
    assert_eq!(f.category, TaskCategory::BugFix);
    assert!(f.file_mentions >= 1);
    assert!(prompt.payload.title.is_none(), "titles are opt-in");
    let test_run = events
        .iter()
        .find(|e| e.payload.tool_use_id.as_deref() == Some("toolu_03"))
        .unwrap();
    assert_eq!(
        test_run.payload.activity,
        Some(ActivityCategory::RunningTests)
    );
    let plan = events
        .iter()
        .rev()
        .find(|e| e.event_type == PlanUpdated)
        .unwrap();
    assert_eq!(plan.payload.plan.as_ref().unwrap().completed_steps, 3);
    assert!(events
        .iter()
        .all(|e| e.source_session_id == "4f1c2a7e-9d3b-4c1e-8a55-2b7f0c9e6d10"));
    let repo = prompt.payload.repository.as_ref().unwrap();
    assert!(repo.id.starts_with("repo_"));
    assert_eq!(repo.display_name.as_deref(), Some("acme-web"));
    let ids: std::collections::HashSet<_> = events.iter().map(|e| e.event_id.clone()).collect();
    assert_eq!(ids.len(), events.len(), "event ids are unique within a run");
}

#[test]
fn claude_titles_when_opted_in() {
    let (events, _) = replay(AgentSource::ClaudeCode, "claude/basic-run.jsonl", true);
    let prompt = events
        .iter()
        .find(|e| e.event_type == EventType::PromptSubmitted)
        .unwrap();
    assert!(prompt
        .payload
        .title
        .as_deref()
        .unwrap()
        .starts_with("Fix the failing login test"));
}

#[test]
fn claude_interrupt_and_failure() {
    let (events, _) = replay(AgentSource::ClaudeCode, "claude/interrupted.jsonl", false);
    let failed = events
        .iter()
        .find(|e| e.event_type == EventType::ToolFailed)
        .unwrap();
    assert_eq!(failed.payload.is_interrupt, Some(true));
    assert_eq!(failed.payload.activity, Some(ActivityCategory::Building));
    let (events, _) = replay(AgentSource::ClaudeCode, "claude/stop-failure.jsonl", false);
    let end = events.last().unwrap();
    assert_eq!(end.event_type, EventType::RunFailed);
    assert_eq!(end.payload.reason.as_deref(), Some("rate_limit"));
}

#[test]
fn claude_task_tools_report_one_task_at_a_time() {
    let (events, _) = replay(AgentSource::ClaudeCode, "claude/task-tools.jsonl", false);
    let changes: Vec<(&str, &str)> = events
        .iter()
        .filter(|e| e.event_type == EventType::PlanUpdated)
        .map(|e| {
            assert!(e.payload.plan.is_none());
            (
                e.payload.task_id.as_deref().unwrap(),
                e.payload.status.as_deref().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        vec![
            ("1", "pending"),
            ("2", "pending"),
            ("3", "pending"),
            ("1", "in_progress"),
            ("1", "completed"),
            ("3", "deleted"),
            ("2", "in_progress"),
            ("2", "completed"),
            ("9", "completed"),
        ]
    );
    let json = serde_json::to_string(&events).unwrap();
    assert!(!json.contains("tax helper"), "task subjects are never sent");
}

#[test]
fn claude_ignores_payloads_from_cursor() {
    let adapter = for_source(AgentSource::ClaudeCode);
    for line in fixture("claude/via-cursor.jsonl") {
        let n = adapter.normalize(&ctx(false), None, line.as_bytes());
        assert!(n.events.is_empty());
        assert_eq!(n.ignored.as_deref(), Some("cursor_compat_hook"));
    }
}

#[test]
fn cursor_basic_run() {
    let (events, outs) = replay(AgentSource::Cursor, "cursor/basic-run.jsonl", false);
    use EventType::*;
    assert_eq!(
        types(&events),
        vec![
            SessionStarted,
            PromptSubmitted,
            ToolCompleted,
            ToolCompleted,
            ToolFailed,
            ToolFailed,
            SubagentCompleted,
            RunCompleted,
            SessionEnded
        ]
    );
    assert_eq!(outs[1], Some("{\"continue\":true}"));
    assert_eq!(outs[0], Some("{}"));
    assert_eq!(
        outs.last().copied().flatten(),
        None,
        "sessionEnd prints nothing"
    );
    let shell = events
        .iter()
        .find(|e| e.payload.tool_use_id.as_deref() == Some("call_3"))
        .unwrap();
    assert_eq!(shell.payload.exit_code, Some(1));
    assert_eq!(shell.payload.activity, Some(ActivityCategory::RunningTests));
    assert!(events
        .iter()
        .filter(|e| e.event_type != SessionStarted && e.event_type != SessionEnded)
        .all(|e| e.source_turn_id.as_deref() == Some("gen-1")));
    assert_eq!(
        events[1].payload.prompt.as_ref().unwrap().category,
        TaskCategory::UiDevelopment
    );
}

#[test]
fn cursor_aborted_is_cancelled_not_completed() {
    let (events, _) = replay(AgentSource::Cursor, "cursor/aborted.jsonl", false);
    assert_eq!(events.last().unwrap().event_type, EventType::RunCancelled);
    assert_eq!(
        events[0].payload.prompt.as_ref().unwrap().category,
        TaskCategory::Migration
    );
}

#[test]
fn cursor_always_acknowledges_even_bad_input() {
    let adapter = for_source(AgentSource::Cursor);
    let n = adapter.normalize(&ctx(false), Some("beforeSubmitPrompt"), b"not json");
    assert_eq!(n.stdout, Some("{\"continue\":true}"));
    assert!(n.events.is_empty());
}

#[test]
fn codex_hooks_run() {
    let (events, outs) = replay(AgentSource::Codex, "codex/hooks-run.jsonl", false);
    use EventType::*;
    assert_eq!(
        types(&events),
        vec![
            SessionStarted,
            PromptSubmitted,
            ToolStarted,
            ToolCompleted,
            ApprovalRequested,
            ToolStarted,
            ToolFailed,
            ToolCompleted,
            PlanUpdated,
            RunCompleted
        ]
    );
    assert!(outs.iter().all(Option::is_none));
    assert!(events[1..]
        .iter()
        .all(|e| e.source_turn_id.as_deref() == Some("turn-1")));
    assert_eq!(events[1].payload.model.as_deref(), Some("gpt-5.1-codex"));
    assert_eq!(events[6].payload.exit_code, Some(101));
    assert_eq!(events[8].payload.plan.as_ref().unwrap().total_steps, 2);
}

#[test]
fn codex_exec_stream() {
    let c = ctx(false);
    let mut stream = ExecStream::new(Some("/home/dev/projects/cli"), &c);
    let events: Vec<EventEnvelope> = fixture("codex/exec-stream.jsonl")
        .iter()
        .flat_map(|l| stream.line(&c, l))
        .collect();
    use EventType::*;
    assert_eq!(
        types(&events),
        vec![
            SessionStarted,
            RunStarted,
            ToolStarted,
            ToolCompleted,
            PlanUpdated,
            ToolStarted,
            ToolFailed,
            ToolCompleted,
            RunCompleted
        ]
    );
    assert!(events
        .iter()
        .all(|e| e.source_session_id == "0199a213-81c0-7800-8aa1-bbab2a035a53"));
    assert_eq!(
        events[5].payload.activity,
        Some(ActivityCategory::RunningTests)
    );
    let ids: std::collections::HashSet<_> = events.iter().map(|e| e.event_id.clone()).collect();
    assert_eq!(ids.len(), events.len());
    assert!(stream.line(&c, "plain text output").is_empty());
}

#[test]
fn malformed_and_hostile_payloads_never_panic() {
    let hostile: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        br#"{"hook_event_name": 5}"#.to_vec(),
        br#"{"hook_event_name":"PreToolUse","session_id":"x","tool_name":{"a":1}}"#.to_vec(),
        format!(r#"{{"hook_event_name":"UserPromptSubmit","session_id":"{}","prompt":"x"}}"#, "s".repeat(10_000)).into_bytes(),
        br#"{"hook_event_name":"PreToolUse","session_id":"../../etc/passwd; rm -rf /","tool_name":"Bash$(id)"}"#.to_vec(),
        vec![0xff, 0xfe, 0x00],
    ];
    for source in AgentSource::ALL {
        let adapter = for_source(source);
        for raw in &hostile {
            let n = adapter.normalize(&ctx(false), Some("PreToolUse"), raw);
            for e in n.events {
                e.validate(Utc::now())
                    .expect("anything emitted must validate");
            }
        }
    }
}

fn opts(home: &Path) -> InstallOptions {
    let exe = home.join("bin").join(if cfg!(windows) {
        "runmarq.exe"
    } else {
        "runmarq"
    });
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"").unwrap();
    InstallOptions {
        scope: Scope::User,
        executable: exe,
        backups_dir: home.join("backups"),
        home: Some(home.to_path_buf()),
    }
}

fn roundtrip(source: AgentSource, existing: &str, rel_config: &str) {
    let dir = tempfile::tempdir().unwrap();
    let home: PathBuf = dir.path().to_path_buf();
    let o = opts(&home);
    let adapter = for_source(source);
    let cfg = home.join(rel_config);
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(&cfg, existing).unwrap();
    let original: serde_json::Value = serde_json::from_str(existing).unwrap();

    let plan = adapter.plan(&o).unwrap();
    assert!(!plan.already_installed);
    assert_eq!(plan.config_path, cfg);

    let r = adapter.install(&o).unwrap();
    assert!(r.changed);
    assert!(r.backup_path.as_ref().unwrap().exists(), "backup written");
    assert!(
        adapter.verify(&o).complete,
        "{:?}",
        adapter.verify(&o).checks
    );
    assert!(adapter.plan(&o).unwrap().already_installed);

    let again = adapter.install(&o).unwrap();
    assert!(!again.changed, "install is idempotent");

    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(
        after["theme"], original["theme"],
        "unrelated settings preserved"
    );

    let u = adapter.uninstall(&o).unwrap();
    assert!(u.removed_entries > 0);
    let restored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(
        restored, original,
        "uninstall restores exactly the original content"
    );
    assert!(!adapter.verify(&o).configured);
    assert_eq!(adapter.uninstall(&o).unwrap().removed_entries, 0);
}

#[test]
fn claude_install_roundtrip_preserves_user_hooks() {
    roundtrip(
        AgentSource::ClaudeCode,
        r#"{"theme":"dark","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"./guard.sh"}]}]}}"#,
        ".claude/settings.json",
    );
}

#[test]
fn codex_install_roundtrip() {
    roundtrip(
        AgentSource::Codex,
        r#"{"theme":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#,
        ".codex/hooks.json",
    );
}

#[test]
fn cursor_install_roundtrip() {
    roundtrip(
        AgentSource::Cursor,
        r#"{"version":1,"theme":"y","hooks":{"stop":[{"command":"./audit.sh"}]}}"#,
        ".cursor/hooks.json",
    );
}

#[test]
fn setup_refuses_corrupt_config() {
    let dir = tempfile::tempdir().unwrap();
    let o = opts(dir.path());
    let cfg = dir.path().join(".claude/settings.json");
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(&cfg, "{ \"theme\": ").unwrap();
    assert!(for_source(AgentSource::ClaudeCode).install(&o).is_err());
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        "{ \"theme\": ",
        "corrupt file untouched"
    );
}

#[test]
fn project_scope_uses_local_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = opts(dir.path());
    o.scope = Scope::Project {
        root: dir.path().join("proj"),
    };
    let a = for_source(AgentSource::ClaudeCode);
    a.install(&o).unwrap();
    assert!(dir.path().join("proj/.claude/settings.local.json").exists());
    assert!(
        !dir.path().join("proj/.claude/settings.json").exists(),
        "shared settings untouched"
    );
}
