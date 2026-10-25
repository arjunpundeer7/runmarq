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

/// When a usage limit resets, read from Claude Code's message such as
/// "You've hit your session limit · resets 3:45pm". The time is in the
/// computer's own time zone. Older versions end the message with
/// `|<unix seconds>` instead.
fn limit_resets_at<Tz: chrono::TimeZone>(
    text: &str,
    now: chrono::DateTime<Tz>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{Datelike, Utc};
    if text.len() > 2000 {
        return None;
    }
    if let Some(secs) = text
        .rsplit_once('|')
        .and_then(|(_, s)| s.trim().parse::<i64>().ok())
        .filter(|s| *s > 1_000_000_000)
    {
        return chrono::DateTime::from_timestamp(secs, 0);
    }
    let lower = text.to_ascii_lowercase();
    let rest = &lower[lower.find("resets ")? + "resets ".len()..];
    let mut words = rest.split_whitespace().peekable();
    let weekday = words.peek().and_then(|w| w.parse::<chrono::Weekday>().ok());
    if weekday.is_some() {
        words.next();
    }
    let clock = words.next()?;
    let (digits, suffix) = clock.split_at(
        clock
            .find(|c: char| !(c.is_ascii_digit() || c == ':'))
            .unwrap_or(clock.len()),
    );
    let meridiem = |s: &str| {
        if s.starts_with("am") {
            Some(0)
        } else if s.starts_with("pm") {
            Some(12)
        } else {
            None
        }
    };
    let offset = match meridiem(suffix) {
        Some(add) => Some(add),
        None if suffix.is_empty() => words.next().and_then(meridiem),
        None if suffix.starts_with(|c: char| c.is_ascii_alphabetic()) => return None,
        None => None,
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    let hour = match offset {
        Some(add) if (1..=12).contains(&h) => h % 12 + add,
        None if h < 24 => h,
        _ => return None,
    };
    let time = chrono::NaiveTime::from_hms_opt(hour, m, 0)?;
    // A reset that has only just passed is the one the message meant.
    let earliest = now.with_timezone(&Utc) - chrono::Duration::minutes(10);
    let tz = now.timezone();
    (0..8)
        .map(|d| now.date_naive() + chrono::Days::new(d))
        .filter(|d| weekday.is_none_or(|w| d.weekday() == w))
        .filter_map(|d| tz.from_local_datetime(&d.and_time(time)).earliest())
        .map(|t| t.with_timezone(&Utc))
        .find(|t| *t > earliest)
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
                if payload.reason.as_deref() == Some("rate_limit") {
                    let now = ctx.now.with_timezone(&chrono::Local);
                    payload.limit_resets_at = ["last_assistant_message", "error_details"]
                        .into_iter()
                        .filter_map(|k| str_field(&v, k))
                        .find_map(|text| limit_resets_at(text, now));
                }
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

#[cfg(test)]
mod tests {
    use super::limit_resets_at;
    use chrono::{DateTime, FixedOffset, TimeZone, Utc};

    fn at(offset_hours: i32, rfc: &str) -> DateTime<FixedOffset> {
        let tz = FixedOffset::east_opt(offset_hours * 3600).unwrap();
        tz.from_local_datetime(&rfc.parse().unwrap()).unwrap()
    }

    fn utc(rfc: &str) -> DateTime<Utc> {
        rfc.parse().unwrap()
    }

    #[test]
    fn reads_the_reset_time_from_the_limit_message() {
        let now = at(0, "2026-10-25T13:00:00");
        let read = |text: &str| limit_resets_at(text, now);
        assert_eq!(
            read("You've hit your session limit · resets 3:45pm"),
            Some(utc("2026-10-25T15:45:00Z"))
        );
        assert_eq!(
            read("5-hour limit reached ∙ resets 3pm (Europe/London)"),
            Some(utc("2026-10-25T15:00:00Z"))
        );
        assert_eq!(
            read("Usage limit reached · resets 9:30 AM"),
            Some(utc("2026-10-26T09:30:00Z")),
            "a time already past today is tomorrow"
        );
        assert_eq!(read("resets 15:30."), Some(utc("2026-10-25T15:30:00Z")));
        assert_eq!(
            read("You've hit your weekly limit · resets Mon 12:00am"),
            Some(utc("2026-10-26T00:00:00Z"))
        );
        assert_eq!(
            read("Claude AI usage limit reached|1793000000"),
            DateTime::from_timestamp(1_793_000_000, 0)
        );
        assert_eq!(read("API Error: Rate limit reached"), None);
        assert_eq!(read("429 Too Many Requests"), None);
        assert_eq!(read("resets 13pm"), None);
    }

    #[test]
    fn reset_times_are_in_the_computers_time_zone() {
        let now = at(5, "2026-10-25T13:00:00");
        assert_eq!(
            limit_resets_at("You've hit your session limit · resets 3:45pm", now),
            Some(utc("2026-10-25T10:45:00Z"))
        );
        let just_after = at(0, "2026-10-25T15:46:00");
        assert_eq!(
            limit_resets_at("resets 3:45pm", just_after),
            Some(utc("2026-10-25T15:45:00Z")),
            "a reset that has only just passed is not moved to tomorrow"
        );
    }
}
