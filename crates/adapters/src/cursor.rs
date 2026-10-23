//! Cursor adapter.
//!
//! Mechanism: Cursor agent hooks in `~/.cursor/hooks.json` or
//! `<project>/.cursor/hooks.json` (https://cursor.com/docs/hooks).
//!
//! runmarq registers only hooks that cannot change Cursor's behaviour.
//! Cursor's permission hooks (`preToolUse`, `beforeShellExecution`,
//! `beforeMCPExecution`, `beforeReadFile`, `subagentStart`) block the action
//! when their output does not match the expected schema, and an explicit
//! `allow` may bypass the user's normal approval prompts. An observer must
//! not take either risk, so tool *start* and approval events are not
//! available for Cursor. Tool completion, failures, prompts and turn end are.
//!
//! The documented acknowledgement for `beforeSubmitPrompt` is
//! `{"continue": true}`; observer hooks with optional output answer `{}`.

use crate::common::{
    classify_tool, envelope, exit_code, extract_plan, hook_executable, prompt_payload,
    repository_ref, safe_id, safe_keyword, safe_text, safe_tool_name, str_field, tool_details,
    with_session_context, ShellStyle,
};
use crate::detect::{find_executable, parse_version, run_with_timeout, Detection};
use crate::{
    config, Adapter, AdapterError, Check, InstallOptions, InstallPlan, InstallResult,
    NormalizeContext, Normalized, Result, Scope, UninstallResult, Verification,
};
use runmarq_core::capabilities::{AgentCapabilities, SupportLevel};
use runmarq_core::event::{ActivityCategory, AgentSource, EventPayload, EventType};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Cursor;

const SOURCE: AgentSource = AgentSource::Cursor;
const HOOK_TIMEOUT_SECONDS: u64 = 10;

/// Observer-safe Cursor hook events and the exact stdout each requires.
pub const EVENTS: &[(&str, Option<&str>)] = &[
    ("sessionStart", Some("{}")),
    ("beforeSubmitPrompt", Some("{\"continue\":true}")),
    ("postToolUse", Some("{}")),
    ("postToolUseFailure", Some("{}")),
    ("subagentStop", Some("{}")),
    ("stop", Some("{}")),
    ("sessionEnd", None),
];

pub fn hook_command(exe: &Path) -> String {
    let style = if cfg!(windows) {
        ShellStyle::WindowsAny
    } else {
        ShellStyle::PosixOrPowerShell
    };
    format!(
        "{} emit --source {}",
        hook_executable(exe, style),
        SOURCE.as_str()
    )
}

/// Each Cursor hook names its event on the command line, so `emit` can print
/// the right acknowledgement even when a payload cannot be parsed.
pub fn event_command(base: &str, event: &str) -> String {
    format!("{base} --event {event}")
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| crate::common::is_runmarq_command(c, SOURCE))
}

fn strip(hooks: &mut Map<String, Value>) -> usize {
    let mut removed = 0;
    let mut empty = Vec::new();
    for (event, list) in hooks.iter_mut() {
        if let Some(list) = list.as_array_mut() {
            let before = list.len();
            list.retain(|e| !is_ours(e));
            let n = before - list.len();
            removed += n;
            if n > 0 && list.is_empty() {
                empty.push(event.clone());
            }
        }
    }
    for e in empty {
        hooks.remove(&e);
    }
    removed
}

fn hooks_mut(root: &mut Map<String, Value>) -> &mut Map<String, Value> {
    if !root.contains_key("version") {
        root.insert("version".into(), json!(1));
    }
    config::hooks_object(root)
}

fn apply(root: &mut Map<String, Value>, command: &str) -> Result<()> {
    let hooks = hooks_mut(root);
    strip(hooks);
    for (event, _) in EVENTS {
        let entry = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
        let Some(list) = entry.as_array_mut() else {
            return Err(AdapterError::Unsupported(format!(
                "hooks.{event} is not a list; fix it manually before setup"
            )));
        };
        list.push(
            json!({ "command": event_command(command, event), "timeout": HOOK_TIMEOUT_SECONDS }),
        );
    }
    Ok(())
}

fn installed_events(root: &Map<String, Value>) -> Vec<(&'static str, Option<String>)> {
    let hooks = root.get("hooks").and_then(Value::as_object);
    EVENTS
        .iter()
        .map(|(event, _)| {
            let cmd = hooks
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|e| is_ours(e))
                .and_then(|e| e.get("command").and_then(Value::as_str).map(String::from));
            (*event, cmd)
        })
        .collect()
}

impl Adapter for Cursor {
    fn source(&self) -> AgentSource {
        SOURCE
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            prompt_start: true,
            turn_completion: true,
            tool_started: false,
            tool_completed: true,
            plan_updates: true,
            approval_events: false,
            error_events: true,
            subagents: true,
            model_metadata: true,
            transcript_usage: crate::transcript::TranscriptFormat::for_source(SOURCE).is_some(),
            interrupt_events: true,
        }
    }

    fn support(&self) -> SupportLevel {
        SupportLevel::Limited
    }

    fn limitations(&self) -> Vec<String> {
        vec![
            "Tool start and approval events are not collected: Cursor's pre-execution hooks can block or auto-approve actions, so runmarq does not register them.".into(),
            "Cursor does not document which shell runs hooks on Windows. Verify the integration after setup.".into(),
            "Project hooks run only in trusted workspaces. Integrated-terminal commands you run yourself are not agent activity and are not tracked.".into(),
            "Cursor does not write a session transcript runmarq can read, so its runs have no token counts.".into(),
        ]
    }

    fn detect(&self, home: &Path) -> Detection {
        let exe = find_executable(&["cursor"]);
        let dir = home.join(".cursor");
        let version = exe
            .as_ref()
            .and_then(|e| run_with_timeout(e, &["--version"], Duration::from_secs(10)))
            .and_then(|s| parse_version(&s));
        Detection {
            detected: exe.is_some() || dir.is_dir(),
            executable: exe,
            version,
            config_dir: dir.is_dir().then_some(dir),
            notes: vec![],
        }
    }

    fn config_path(&self, scope: &Scope, home: &Path) -> PathBuf {
        match scope {
            Scope::User => home.join(".cursor").join("hooks.json"),
            Scope::Project { root } => root.join(".cursor").join("hooks.json"),
        }
    }

    fn plan(&self, opts: &InstallOptions) -> Result<InstallPlan> {
        let path = self.config_path(&opts.scope, &opts.home());
        let command = hook_command(&opts.executable);
        let existing = config::read_object(&path)?;
        let already = existing
            .as_ref()
            .map(|r| {
                installed_events(r)
                    .iter()
                    .all(|(e, c)| c.as_deref() == Some(event_command(&command, e).as_str()))
            })
            .unwrap_or(false);
        let mut preview = Map::new();
        apply(&mut preview, &command)?;
        Ok(InstallPlan {
            agent: SOURCE,
            file_exists: existing.is_some(),
            config_path: path,
            already_installed: already,
            events: EVENTS.iter().map(|(e, _)| e.to_string()).collect(),
            command,
            preview: Value::Object(preview),
            notes: vec![
                "Only after-the-fact hooks are registered; none can block or approve agent actions.".into(),
                "Existing hooks are preserved. A backup is written before any change.".into(),
                "Cursor reloads hooks.json automatically; restart Cursor if the Hooks panel does not show runmarq.".into(),
            ],
        })
    }

    fn install(&self, opts: &InstallOptions) -> Result<InstallResult> {
        let path = self.config_path(&opts.scope, &opts.home());
        let original = config::read_object(&path)?;
        let mut root = original.clone().unwrap_or_default();
        apply(&mut root, &hook_command(&opts.executable))?;
        if original.as_ref() == Some(&root) {
            return Ok(InstallResult {
                config_path: path,
                backup_path: None,
                changed: false,
                pending_steps: vec![],
            });
        }
        let backup = config::backup(&path, &opts.backups_dir, SOURCE.as_str())?;
        config::write_object(&path, &root)?;
        Ok(InstallResult {
            config_path: path,
            backup_path: backup,
            changed: true,
            pending_steps: vec![],
        })
    }

    fn verify(&self, opts: &InstallOptions) -> Verification {
        let path = self.config_path(&opts.scope, &opts.home());
        let mut checks = Vec::new();
        let root = match config::read_object(&path) {
            Ok(r) => r,
            Err(e) => {
                checks.push(Check {
                    label: "Configuration file readable".into(),
                    ok: false,
                    detail: Some(e.to_string()),
                });
                return Verification {
                    config_path: path,
                    configured: false,
                    complete: false,
                    checks,
                    pending_steps: vec![],
                };
            }
        };
        checks.push(Check {
            label: "Configuration file readable".into(),
            ok: true,
            detail: Some(path.display().to_string()),
        });
        let found = root
            .as_ref()
            .map(installed_events)
            .unwrap_or_else(|| EVENTS.iter().map(|(e, _)| (*e, None)).collect());
        let missing: Vec<&str> = found
            .iter()
            .filter(|(_, c)| c.is_none())
            .map(|(e, _)| *e)
            .collect();
        let version_ok = root
            .as_ref()
            .and_then(|r| r.get("version"))
            .and_then(Value::as_u64)
            .is_some_and(|v| v >= 1);
        checks.push(Check {
            label: "runmarq hooks registered".into(),
            ok: missing.is_empty(),
            detail: Some(if missing.is_empty() {
                format!("{} events", EVENTS.len())
            } else {
                format!("missing: {}", missing.join(", "))
            }),
        });
        if root.is_some() {
            checks.push(Check {
                label: "hooks.json declares a schema version".into(),
                ok: version_ok,
                detail: None,
            });
        }
        let exe_ok = opts.executable.is_file();
        checks.push(Check {
            label: "Hook executable present".into(),
            ok: exe_ok,
            detail: Some(opts.executable.display().to_string()),
        });
        Verification {
            config_path: path,
            configured: missing.len() < EVENTS.len(),
            complete: missing.is_empty() && exe_ok && version_ok,
            checks,
            pending_steps: vec![],
        }
    }

    fn uninstall(&self, opts: &InstallOptions) -> Result<UninstallResult> {
        let path = self.config_path(&opts.scope, &opts.home());
        let Some(mut root) = config::read_object(&path)? else {
            return Ok(UninstallResult {
                config_path: path,
                backup_path: None,
                removed_entries: 0,
            });
        };
        let removed = root
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .map(strip)
            .unwrap_or(0);
        if removed == 0 {
            return Ok(UninstallResult {
                config_path: path,
                backup_path: None,
                removed_entries: 0,
            });
        }
        let backup = config::backup(&path, &opts.backups_dir, SOURCE.as_str())?;
        config::write_object(&path, &root)?;
        Ok(UninstallResult {
            config_path: path,
            backup_path: backup,
            removed_entries: removed,
        })
    }

    fn normalize(
        &self,
        ctx: &NormalizeContext,
        declared_event: Option<&str>,
        raw: &[u8],
    ) -> Normalized {
        let event_name = |v: Option<&Value>| {
            v.and_then(|v| str_field(v, "hook_event_name"))
                .or(declared_event)
                .map(String::from)
        };
        let parsed = serde_json::from_slice::<Value>(raw).ok();
        let event = event_name(parsed.as_ref());
        let stdout = event
            .as_deref()
            .and_then(|e| EVENTS.iter().find(|(n, _)| *n == e))
            .and_then(|(_, out)| *out);
        let Some(v) = parsed else {
            return Normalized {
                stdout,
                ignored: Some("invalid_json".into()),
                ..Default::default()
            };
        };
        let Some(event) = event else {
            return Normalized {
                stdout,
                ignored: Some("missing_event_name".into()),
                ..Default::default()
            };
        };
        let Some(session) = str_field(&v, "conversation_id")
            .or_else(|| str_field(&v, "session_id"))
            .and_then(safe_id)
        else {
            return Normalized {
                stdout,
                ignored: Some("missing_conversation_id".into()),
                ..Default::default()
            };
        };
        let turn = str_field(&v, "generation_id").and_then(safe_id);
        let cwd = v
            .get("workspace_roots")
            .and_then(Value::as_array)
            .and_then(|r| r.first())
            .and_then(Value::as_str)
            .or_else(|| str_field(&v, "cwd"));

        let mut payload = EventPayload {
            repository: cwd.and_then(|c| repository_ref(c, ctx)),
            model: str_field(&v, "model").and_then(|m| safe_text(m, 96)),
            agent_version: str_field(&v, "cursor_version").and_then(|m| safe_text(m, 32)),
            ..Default::default()
        };
        let tool_name = str_field(&v, "tool_name");
        let tool_input = v.get("tool_input");
        if let Some(t) = tool_name {
            payload.tool_name = safe_tool_name(t);
            payload.activity = Some(classify_tool(t, tool_input));
        }
        payload.tool_use_id = str_field(&v, "tool_use_id").and_then(safe_id);
        if let Some(t) = tool_name {
            tool_details(t, tool_input, ctx, &mut payload);
        }
        payload.duration_ms = v
            .get("duration")
            .or_else(|| v.get("duration_ms"))
            .and_then(Value::as_u64);

        let mut events = Vec::new();
        match event.as_str() {
            "sessionStart" => events.push((EventType::SessionStarted, payload)),
            "sessionEnd" => {
                payload.reason = str_field(&v, "reason").and_then(safe_keyword);
                events.push((EventType::SessionEnded, payload));
            }
            "beforeSubmitPrompt" => {
                let mut p = prompt_payload(str_field(&v, "prompt"), ctx);
                p.repository = payload.repository;
                p.model = payload.model;
                p.agent_version = payload.agent_version;
                events.push((EventType::PromptSubmitted, p));
            }
            "postToolUse" => {
                let plan = tool_name.and_then(|t| extract_plan(t, tool_input));
                payload.exit_code = exit_code(v.get("tool_output"));
                let failed = payload.exit_code.is_some_and(|c| c != 0);
                events.push((
                    if failed {
                        EventType::ToolFailed
                    } else {
                        EventType::ToolCompleted
                    },
                    payload,
                ));
                if let Some(plan) = plan {
                    events.push((
                        EventType::PlanUpdated,
                        EventPayload {
                            plan: Some(plan),
                            ..Default::default()
                        },
                    ));
                }
            }
            "postToolUseFailure" => {
                payload.is_interrupt = v.get("is_interrupt").and_then(Value::as_bool);
                payload.reason = str_field(&v, "failure_type").and_then(safe_keyword);
                events.push((EventType::ToolFailed, payload));
            }
            "subagentStop" => {
                payload.activity = Some(ActivityCategory::Delegating);
                payload.status = str_field(&v, "status").and_then(safe_keyword);
                events.push((EventType::SubagentCompleted, payload));
            }
            "stop" => {
                let status = str_field(&v, "status").unwrap_or("completed");
                payload.status = safe_keyword(status);
                let t = match status {
                    "aborted" => EventType::RunCancelled,
                    "error" => EventType::RunFailed,
                    _ => EventType::RunCompleted,
                };
                events.push((t, payload));
            }
            other => {
                return Normalized {
                    stdout,
                    ignored: Some(format!(
                        "unhandled_event:{}",
                        safe_keyword(other).unwrap_or_default()
                    )),
                    ..Default::default()
                }
            }
        }
        with_session_context(&v, &mut events);
        let discriminator = format!("{event}/{}", str_field(&v, "tool_use_id").unwrap_or(""));
        Normalized {
            events: events
                .into_iter()
                .map(|(t, p)| {
                    envelope(
                        SOURCE,
                        ctx,
                        raw,
                        &discriminator,
                        session.clone(),
                        turn.clone(),
                        t,
                        None,
                        p,
                    )
                })
                .collect(),
            stdout,
            ignored: None,
        }
    }
}
