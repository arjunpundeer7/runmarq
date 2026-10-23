//! Claude Code adapter.
//!
//! Mechanism: command hooks in Claude Code `settings.json`
//! (https://code.claude.com/docs/en/hooks). Every hook runs
//! `runmarq emit --source claude_code`, prints nothing, and exits 0, so it can
//! never block a tool call or inject text into the conversation.
//!
//! Event names were checked against the installed 2.1.x binary. Claude Code
//! does not fire `Stop` when the user interrupts a turn; interrupts are
//! inferred from `PostToolUseFailure.is_interrupt`, an idle notification
//! without a preceding `Stop`, or a new prompt in the same session.
//!
//! Cursor also executes hooks from Claude Code settings files. Payloads from
//! Cursor carry `cursor_version`; they are ignored here so that Cursor runs
//! are counted once, through the native Cursor integration.

use crate::common::{
    classify_tool, envelope, exit_code, extract_plan, hook_executable, prompt_payload,
    repository_ref, safe_id, safe_keyword, safe_text, safe_tool_name, str_field, task_change,
    tool_details, with_session_context, ShellStyle,
};
use crate::detect::{find_executable, parse_version, run_with_timeout, Detection};
use crate::grouped::{self, HookSpec};
use crate::{
    config, Adapter, Check, InstallOptions, InstallPlan, InstallResult, NormalizeContext,
    Normalized, Result, Scope, UninstallResult, Verification,
};
use runmarq_core::capabilities::{AgentCapabilities, SupportLevel};
use runmarq_core::event::{ActivityCategory, AgentSource, EventPayload, EventType};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Claude;

pub const SPECS: &[HookSpec] = &[
    HookSpec {
        event: "SessionStart",
        tool_matcher: false,
    },
    HookSpec {
        event: "UserPromptSubmit",
        tool_matcher: false,
    },
    HookSpec {
        event: "PreToolUse",
        tool_matcher: true,
    },
    HookSpec {
        event: "PostToolUse",
        tool_matcher: true,
    },
    HookSpec {
        event: "PostToolUseFailure",
        tool_matcher: true,
    },
    HookSpec {
        event: "PermissionRequest",
        tool_matcher: true,
    },
    HookSpec {
        event: "Notification",
        tool_matcher: false,
    },
    HookSpec {
        event: "SubagentStart",
        tool_matcher: false,
    },
    HookSpec {
        event: "SubagentStop",
        tool_matcher: false,
    },
    HookSpec {
        event: "PreCompact",
        tool_matcher: false,
    },
    HookSpec {
        event: "Stop",
        tool_matcher: false,
    },
    HookSpec {
        event: "StopFailure",
        tool_matcher: false,
    },
    HookSpec {
        event: "SessionEnd",
        tool_matcher: false,
    },
];

const HOOK_TIMEOUT_SECONDS: u64 = 10;
const SOURCE: AgentSource = AgentSource::ClaudeCode;

fn claude_dir(home: &Path) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
}

pub fn hook_command(exe: &Path) -> String {
    format!(
        "{} emit --source {}",
        hook_executable(exe, ShellStyle::PosixOrPowerShell),
        SOURCE.as_str()
    )
}

/// Plan/install/verify/uninstall for the matcher-group schema. Shared with Codex.
pub(crate) mod flow {
    use super::*;

    pub fn plan(
        source: AgentSource,
        path: PathBuf,
        specs: &[HookSpec],
        command: String,
        notes: Vec<String>,
    ) -> Result<InstallPlan> {
        let existing = config::read_object(&path)?;
        let already = existing
            .as_ref()
            .and_then(|o| o.get("hooks").and_then(Value::as_object))
            .map(|h| {
                grouped::installed(h, source, specs)
                    .iter()
                    .all(|(_, c)| c.as_deref() == Some(command.as_str()))
            })
            .unwrap_or(false);
        Ok(InstallPlan {
            agent: source,
            file_exists: existing.is_some(),
            config_path: path,
            already_installed: already,
            events: specs.iter().map(|s| s.event.to_string()).collect(),
            preview: grouped::preview(specs, &command, HOOK_TIMEOUT_SECONDS),
            command,
            notes,
        })
    }

    pub fn install(
        source: AgentSource,
        path: PathBuf,
        specs: &[HookSpec],
        command: &str,
        backups: &Path,
        pending: Vec<String>,
    ) -> Result<InstallResult> {
        let original = config::read_object(&path)?;
        let mut root = original.clone().unwrap_or_default();
        let skipped = grouped::install(
            config::hooks_object(&mut root),
            source,
            specs,
            command,
            HOOK_TIMEOUT_SECONDS,
        );
        if !skipped.is_empty() {
            return Err(crate::AdapterError::Unsupported(format!(
                "{} has a non-list value for {}; fix it manually before setup",
                path.display(),
                skipped.join(", ")
            )));
        }
        if original.as_ref() == Some(&root) {
            return Ok(InstallResult {
                config_path: path,
                backup_path: None,
                changed: false,
                pending_steps: pending,
            });
        }
        let backup = config::backup(&path, backups, source.as_str())?;
        config::write_object(&path, &root)?;
        Ok(InstallResult {
            config_path: path,
            backup_path: backup,
            changed: true,
            pending_steps: pending,
        })
    }

    pub fn uninstall(
        source: AgentSource,
        path: PathBuf,
        backups: &Path,
    ) -> Result<UninstallResult> {
        let Some(mut root) = config::read_object(&path)? else {
            return Ok(UninstallResult {
                config_path: path,
                backup_path: None,
                removed_entries: 0,
            });
        };
        let removed = match root.get_mut("hooks").and_then(Value::as_object_mut) {
            Some(h) => grouped::strip(h, source),
            None => 0,
        };
        if removed == 0 {
            return Ok(UninstallResult {
                config_path: path,
                backup_path: None,
                removed_entries: 0,
            });
        }
        if root
            .get("hooks")
            .and_then(Value::as_object)
            .is_some_and(|h| h.is_empty())
        {
            root.remove("hooks");
        }
        let backup = config::backup(&path, backups, source.as_str())?;
        config::write_object(&path, &root)?;
        Ok(UninstallResult {
            config_path: path,
            backup_path: backup,
            removed_entries: removed,
        })
    }

    pub fn verify(
        source: AgentSource,
        path: PathBuf,
        specs: &[HookSpec],
        exe: &Path,
        pending: Vec<String>,
    ) -> Verification {
        let mut checks = Vec::new();
        let root = match config::read_object(&path) {
            Ok(r) => {
                checks.push(Check {
                    label: "Configuration file readable".into(),
                    ok: true,
                    detail: Some(path.display().to_string()),
                });
                r
            }
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
                    pending_steps: pending,
                };
            }
        };
        let found = root
            .as_ref()
            .and_then(|o| o.get("hooks").and_then(Value::as_object))
            .map(|h| grouped::installed(h, source, specs))
            .unwrap_or_else(|| specs.iter().map(|s| (s.event, None)).collect());
        let missing: Vec<&str> = found
            .iter()
            .filter(|(_, c)| c.is_none())
            .map(|(e, _)| *e)
            .collect();
        let configured = missing.len() < specs.len();
        checks.push(Check {
            label: "runmarq hooks registered".into(),
            ok: missing.is_empty(),
            detail: if missing.is_empty() {
                Some(format!("{} events", specs.len()))
            } else {
                Some(format!("missing: {}", missing.join(", ")))
            },
        });
        let exe_ok = exe.is_file();
        checks.push(Check {
            label: "Hook executable present".into(),
            ok: exe_ok,
            detail: Some(exe.display().to_string()),
        });
        let points_here = found.iter().filter_map(|(_, c)| c.as_deref()).all(|c| {
            let wanted = exe.to_string_lossy().replace('\\', "/").to_lowercase();
            let c = c.replace('\\', "/").to_lowercase();
            c.contains(&wanted)
                || c.contains(&hook_executable(exe, ShellStyle::PosixOrPowerShell).to_lowercase())
                || c.contains(
                    &hook_executable(exe, ShellStyle::WindowsAny)
                        .replace('\\', "/")
                        .to_lowercase(),
                )
        });
        if configured {
            checks.push(Check {
                label: "Hooks point at this runmarq installation".into(),
                ok: points_here,
                detail: (!points_here).then(|| "run setup again to update the path".into()),
            });
        }
        Verification {
            config_path: path,
            configured,
            complete: missing.is_empty() && exe_ok && points_here,
            checks,
            pending_steps: pending,
        }
    }
}

impl Adapter for Claude {
    fn source(&self) -> AgentSource {
        SOURCE
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            prompt_start: true,
            turn_completion: true,
            tool_started: true,
            tool_completed: true,
            plan_updates: true,
            approval_events: true,
            error_events: true,
            subagents: true,
            model_metadata: true,
            transcript_usage: crate::transcript::TranscriptFormat::for_source(SOURCE).is_some(),
            interrupt_events: false,
        }
    }

    fn support(&self) -> SupportLevel {
        SupportLevel::Available
    }

    fn limitations(&self) -> Vec<String> {
        vec![
            "Claude Code does not report user interrupts directly. runmarq infers them and marks those runs cancelled, never finished.".into(),
            "Plan progress is available only when Claude keeps a task list or to-do list.".into(),
            "Model metadata is reported at session start, not for every prompt.".into(),
        ]
    }

    fn detect(&self, home: &Path) -> Detection {
        let exe = find_executable(&["claude"]);
        let dir = claude_dir(home);
        let version = exe
            .as_ref()
            .and_then(|e| run_with_timeout(e, &["--version"], Duration::from_secs(8)))
            .and_then(|s| parse_version(&s));
        Detection {
            detected: exe.is_some() || dir.is_dir(),
            executable: exe,
            version,
            config_dir: dir.is_dir().then_some(dir),
            notes: Vec::new(),
        }
    }

    fn config_path(&self, scope: &Scope, home: &Path) -> PathBuf {
        match scope {
            Scope::User => claude_dir(home).join("settings.json"),
            // settings.local.json is personal and git-ignored by Claude Code,
            // so setup never edits a file the team shares.
            Scope::Project { root } => root.join(".claude").join("settings.local.json"),
        }
    }

    fn plan(&self, opts: &InstallOptions) -> Result<InstallPlan> {
        flow::plan(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            SPECS,
            hook_command(&opts.executable),
            vec![
                "Hooks print nothing and always exit 0, so they cannot block tools or add text to conversations.".into(),
                "Existing hooks and settings are preserved. A backup is written before any change.".into(),
                "Sessions that are already running pick up the hooks after Claude Code reloads settings; new sessions use them immediately.".into(),
            ],
        )
    }

    fn install(&self, opts: &InstallOptions) -> Result<InstallResult> {
        flow::install(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            SPECS,
            &hook_command(&opts.executable),
            &opts.backups_dir,
            vec![],
        )
    }

    fn verify(&self, opts: &InstallOptions) -> Verification {
        flow::verify(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            SPECS,
            &opts.executable,
            vec![],
        )
    }

    fn uninstall(&self, opts: &InstallOptions) -> Result<UninstallResult> {
        flow::uninstall(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            &opts.backups_dir,
        )
    }

    fn normalize(
        &self,
        ctx: &NormalizeContext,
        declared_event: Option<&str>,
        raw: &[u8],
    ) -> Normalized {
        let Ok(v) = serde_json::from_slice::<Value>(raw) else {
            return Normalized {
                ignored: Some("invalid_json".into()),
                ..Default::default()
            };
        };
        if v.get("cursor_version").is_some() {
            return Normalized {
                ignored: Some("cursor_compat_hook".into()),
                ..Default::default()
            };
        }
        let Some(event) = str_field(&v, "hook_event_name").or(declared_event) else {
            return Normalized {
                ignored: Some("missing_event_name".into()),
                ..Default::default()
            };
        };
        let Some(session) = str_field(&v, "session_id").and_then(safe_id) else {
            return Normalized {
                ignored: Some("missing_session_id".into()),
                ..Default::default()
            };
        };

        let mut payload = EventPayload {
            repository: str_field(&v, "cwd").and_then(|c| repository_ref(c, ctx)),
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
        payload.duration_ms = v.get("duration_ms").and_then(Value::as_u64);
        payload.subagent_id = str_field(&v, "agent_id").and_then(safe_id);

        let mut events: Vec<(EventType, EventPayload)> = Vec::new();
        match event {
            "SessionStart" => {
                payload.model = str_field(&v, "model").and_then(|m| safe_text(m, 96));
                payload.reason = str_field(&v, "source").and_then(safe_keyword);
                events.push((EventType::SessionStarted, payload));
            }
            "UserPromptSubmit" => {
                let mut p = prompt_payload(str_field(&v, "prompt"), ctx);
                p.repository = payload.repository;
                events.push((EventType::PromptSubmitted, p));
            }
            "PreToolUse" => events.push((EventType::ToolStarted, payload)),
            "PostToolUse" => {
                let plan = tool_name.and_then(|t| extract_plan(t, tool_input));
                let task =
                    tool_name.and_then(|t| task_change(t, tool_input, v.get("tool_response")));
                payload.exit_code = exit_code(v.get("tool_response"));
                let failed = payload.exit_code.is_some_and(|c| c != 0);
                events.push((
                    if failed {
                        EventType::ToolFailed
                    } else {
                        EventType::ToolCompleted
                    },
                    payload.clone(),
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
                if let Some((task_id, status)) = task {
                    events.push((
                        EventType::PlanUpdated,
                        EventPayload {
                            task_id: Some(task_id),
                            status: Some(status),
                            ..Default::default()
                        },
                    ));
                }
            }
            "PostToolUseFailure" => {
                payload.is_interrupt = v.get("is_interrupt").and_then(Value::as_bool);
                payload.reason = Some("tool_error".into());
                events.push((EventType::ToolFailed, payload));
            }
            "PermissionRequest" => {
                payload.activity = Some(ActivityCategory::WaitingForApproval);
                payload.reason = Some("permission_request".into());
                events.push((EventType::ApprovalRequested, payload));
            }
            "Notification" => match str_field(&v, "notification_type") {
                Some(
                    kind @ ("permission_prompt"
                    | "elicitation_dialog"
                    | "elicitation_url_dialog"
                    | "agent_needs_input"),
                ) => {
                    payload.activity = Some(ActivityCategory::WaitingForApproval);
                    payload.reason = safe_keyword(kind);
                    events.push((EventType::ApprovalRequested, payload));
                }
                Some("idle_prompt") => {
                    payload.reason = Some("idle_prompt".into());
                    events.push((EventType::AgentIdle, payload));
                }
                other => {
                    return Normalized {
                        ignored: Some(format!(
                            "notification:{}",
                            other.and_then(safe_keyword).unwrap_or_default()
                        )),
                        ..Default::default()
                    }
                }
            },
            "SubagentStart" => {
                payload.activity = Some(ActivityCategory::Delegating);
                events.push((EventType::SubagentStarted, payload));
            }
            "SubagentStop" => events.push((EventType::SubagentCompleted, payload)),
            "PreCompact" => {
                payload.activity = Some(ActivityCategory::CompactingContext);
                events.push((EventType::ContextCompacting, payload));
            }
            "Stop" => {
                // Subagents report through SubagentStop. `agent_id` can also be
                // present on the main thread (`claude --agent`), so it does not
                // make a Stop a subagent event.
                payload.subagent_id = None;
                payload.status = Some("completed".into());
                events.push((EventType::RunCompleted, payload));
            }
            "StopFailure" => {
                payload.status = Some("error".into());
                payload.reason = v
                    .get("error")
                    .and_then(|e| e.as_str().or_else(|| e.get("type").and_then(Value::as_str)))
                    .and_then(safe_keyword);
                events.push((EventType::RunFailed, payload));
            }
            "SessionEnd" => {
                payload.reason = str_field(&v, "reason").and_then(safe_keyword);
                events.push((EventType::SessionEnded, payload));
            }
            other => {
                return Normalized {
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
        let envelopes = events
            .into_iter()
            .map(|(t, p)| {
                envelope(
                    SOURCE,
                    ctx,
                    raw,
                    &discriminator,
                    session.clone(),
                    None,
                    t,
                    None,
                    p,
                )
            })
            .collect();
        // Silence on stdout is required: for UserPromptSubmit and SessionStart,
        // Claude Code would add printed text to the conversation.
        Normalized {
            events: envelopes,
            stdout: None,
            ignored: None,
        }
    }
}
