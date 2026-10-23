//! Codex CLI adapter.
//!
//! Two supported mechanisms, both structured:
//!
//! 1. Lifecycle hooks in `~/.codex/hooks.json` (stable and enabled by default
//!    in Codex 0.130). The 0.130 binary defines eight events: SessionStart,
//!    UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse,
//!    PreCompact, PostCompact, Stop. Every payload except SessionStart carries
//!    `turn_id`, which maps one-to-one onto a runmarq run. Codex skips hooks
//!    the user has not trusted, so setup leaves one manual step: approve the
//!    hooks with `/hooks` inside Codex.
//! 2. `codex exec --json` emits a JSONL event stream. `runmarq run -- codex exec --json ...`
//!    reads that stream while passing it through unchanged.
//!
//! Codex 0.130 has no session-end, interrupt or tool-failure hooks. The
//! `runmarq run` wrapper reports process exit, which closes runs left open.
//! Codex runs hook commands through `cmd.exe /C` on Windows and `sh -lc`
//! elsewhere, which determines how the command is quoted.

use crate::claude::flow;
use crate::common::{
    classify_command, classify_tool, envelope, exit_code, extract_plan, hook_executable,
    prompt_payload, repository_ref, safe_id, safe_keyword, safe_text, safe_tool_name, str_field,
    tool_details, with_session_context, ShellStyle,
};
use crate::detect::{
    find_executable, parse_version, run_with_timeout, version_at_least, Detection,
};
use crate::grouped::HookSpec;
use crate::{
    Adapter, InstallOptions, InstallPlan, InstallResult, NormalizeContext, Normalized, Result,
    Scope, UninstallResult, Verification,
};
use runmarq_core::capabilities::{AgentCapabilities, SupportLevel};
use runmarq_core::event::{
    ActivityCategory, AgentSource, EventEnvelope, EventPayload, EventType, PlanSnapshot,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Codex;

const SOURCE: AgentSource = AgentSource::Codex;
pub const MIN_HOOKS_VERSION: &str = "0.130.0";

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
        event: "PermissionRequest",
        tool_matcher: true,
    },
    HookSpec {
        event: "PostToolUse",
        tool_matcher: true,
    },
    HookSpec {
        event: "PreCompact",
        tool_matcher: false,
    },
    HookSpec {
        event: "Stop",
        tool_matcher: false,
    },
];

pub const TRUST_STEP: &str =
    "Open Codex and run /hooks, then trust the runmarq hooks. Codex does not run hooks until they are trusted.";

fn codex_home(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"))
}

pub fn hook_command(exe: &Path) -> String {
    let style = if cfg!(windows) {
        ShellStyle::Cmd
    } else {
        ShellStyle::PosixOrPowerShell
    };
    format!(
        "{} emit --source {}",
        hook_executable(exe, style),
        SOURCE.as_str()
    )
}

impl Adapter for Codex {
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
            error_events: false,
            subagents: false,
            model_metadata: true,
            transcript_usage: crate::transcript::TranscriptFormat::for_source(SOURCE).is_some(),
            interrupt_events: false,
        }
    }

    fn support(&self) -> SupportLevel {
        SupportLevel::Limited
    }

    fn limitations(&self) -> Vec<String> {
        vec![
            "Requires Codex 0.130 or later, and the runmarq hooks must be trusted once with /hooks inside Codex.".into(),
            "Codex 0.130 has no tool-failure, interrupt or session-end hooks. Failed shell commands are detected from exit codes when Codex reports them.".into(),
            "Start Codex with `runmarq run -- codex` to have runs closed when the process exits.".into(),
            "Already-running Codex IDE sessions that were started without hooks cannot be observed.".into(),
        ]
    }

    fn detect(&self, home: &Path) -> Detection {
        let exe = find_executable(&["codex"]);
        let dir = codex_home(home);
        let version = exe
            .as_ref()
            .and_then(|e| run_with_timeout(e, &["--version"], Duration::from_secs(8)))
            .and_then(|s| parse_version(&s));
        let mut notes = Vec::new();
        if let Some(v) = &version {
            if !version_at_least(v, MIN_HOOKS_VERSION) {
                notes.push(format!("Codex {v} predates stable hooks ({MIN_HOOKS_VERSION}). Update Codex to connect it."));
            }
        }
        Detection {
            detected: exe.is_some() || dir.is_dir(),
            executable: exe,
            version,
            config_dir: dir.is_dir().then_some(dir),
            notes,
        }
    }

    fn config_path(&self, scope: &Scope, home: &Path) -> PathBuf {
        match scope {
            Scope::User => codex_home(home).join("hooks.json"),
            Scope::Project { root } => root.join(".codex").join("hooks.json"),
        }
    }

    fn plan(&self, opts: &InstallOptions) -> Result<InstallPlan> {
        flow::plan(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            SPECS,
            hook_command(&opts.executable),
            vec![
                "runmarq writes only hooks.json; config.toml is never modified.".into(),
                "Hooks print nothing and always exit 0, so they cannot block tools or add context."
                    .into(),
                TRUST_STEP.into(),
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
            vec![TRUST_STEP.into()],
        )
    }

    fn verify(&self, opts: &InstallOptions) -> Verification {
        flow::verify(
            SOURCE,
            self.config_path(&opts.scope, &opts.home()),
            SPECS,
            &opts.executable,
            vec![TRUST_STEP.into()],
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
        let turn = str_field(&v, "turn_id").and_then(safe_id);

        let mut payload = EventPayload {
            repository: str_field(&v, "cwd").and_then(|c| repository_ref(c, ctx)),
            model: str_field(&v, "model").and_then(|m| safe_text(m, 96)),
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

        let mut events = Vec::new();
        match event {
            "SessionStart" => {
                payload.reason = str_field(&v, "source").and_then(safe_keyword);
                events.push((EventType::SessionStarted, payload));
            }
            "UserPromptSubmit" => {
                let mut p = prompt_payload(str_field(&v, "prompt"), ctx);
                p.repository = payload.repository;
                p.model = payload.model;
                events.push((EventType::PromptSubmitted, p));
            }
            "PreToolUse" => events.push((EventType::ToolStarted, payload)),
            "PermissionRequest" => {
                payload.activity = Some(ActivityCategory::WaitingForApproval);
                payload.reason = Some("permission_request".into());
                events.push((EventType::ApprovalRequested, payload));
            }
            "PostToolUse" => {
                let plan = tool_name.and_then(|t| extract_plan(t, tool_input));
                payload.exit_code = exit_code(v.get("tool_response"));
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
            "PreCompact" => {
                payload.activity = Some(ActivityCategory::CompactingContext);
                events.push((EventType::ContextCompacting, payload));
            }
            "PostCompact" => events.push((EventType::Heartbeat, payload)),
            "Stop" => {
                payload.status = Some("completed".into());
                events.push((EventType::RunCompleted, payload));
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
            stdout: None,
            ignored: None,
        }
    }
}

/// Incremental parser for the `codex exec --json` JSONL stream.
#[derive(Debug, Default)]
pub struct ExecStream {
    thread_id: Option<String>,
    turn_counter: u32,
    turn_id: Option<String>,
    repository: Option<runmarq_core::event::RepositoryRef>,
    /// Line sequence number. Identical lines (two `turn.started`, say) are
    /// distinct events, so the id must not depend on content alone.
    seq: u64,
}

impl ExecStream {
    pub fn new(cwd: Option<&str>, ctx: &NormalizeContext) -> ExecStream {
        ExecStream {
            repository: cwd.and_then(|c| repository_ref(c, ctx)),
            ..Default::default()
        }
    }

    /// Translates one stdout line. Lines that are not Codex events are ignored.
    pub fn line(&mut self, ctx: &NormalizeContext, line: &str) -> Vec<EventEnvelope> {
        let line = line.trim();
        if !line.starts_with('{') || line.len() > 1_000_000 {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let Some(kind) = str_field(&v, "type") else {
            return Vec::new();
        };
        self.seq += 1;
        let mut out = Vec::new();
        let base = || EventPayload {
            repository: self.repository.clone(),
            ..Default::default()
        };
        match kind {
            "thread.started" => {
                self.thread_id = str_field(&v, "thread_id").and_then(safe_id);
                out.push((EventType::SessionStarted, base()));
            }
            "turn.started" => {
                self.turn_counter += 1;
                self.turn_id = Some(format!("exec-turn-{}", self.turn_counter));
                out.push((EventType::RunStarted, base()));
            }
            "turn.completed" => {
                let mut p = base();
                p.status = Some("completed".into());
                out.push((EventType::RunCompleted, p));
            }
            "turn.failed" => {
                let mut p = base();
                p.status = Some("error".into());
                p.reason = Some("turn_failed".into());
                out.push((EventType::RunFailed, p));
            }
            "item.started" | "item.completed" => {
                let Some(item) = v.get("item") else {
                    return Vec::new();
                };
                let started = kind == "item.started";
                let item_type = str_field(item, "type").unwrap_or("");
                let mut p = base();
                p.tool_use_id = str_field(item, "id").and_then(safe_id);
                let (tool, activity) = match item_type {
                    "command_execution" => (
                        "shell",
                        str_field(item, "command")
                            .map(classify_command)
                            .unwrap_or(ActivityCategory::RunningCommands),
                    ),
                    "file_change" => ("apply_patch", ActivityCategory::EditingCode),
                    "mcp_tool_call" => ("mcp", ActivityCategory::Research),
                    "web_search" => ("web_search", ActivityCategory::Research),
                    "collab_tool_call" => ("spawn_agent", ActivityCategory::Delegating),
                    "todo_list" => {
                        if let Some(items) = item.get("items").and_then(Value::as_array) {
                            let done = items
                                .iter()
                                .filter(|i| {
                                    i.get("completed").and_then(Value::as_bool) == Some(true)
                                })
                                .count();
                            p.plan = Some(PlanSnapshot {
                                total_steps: items.len().min(500) as u32,
                                completed_steps: done.min(500) as u32,
                            });
                            out.push((EventType::PlanUpdated, p));
                        }
                        return self.wrap(ctx, line, out);
                    }
                    _ => return Vec::new(),
                };
                p.tool_name = safe_tool_name(tool);
                p.activity = Some(activity);
                if started {
                    out.push((EventType::ToolStarted, p));
                } else {
                    let status = str_field(item, "status").unwrap_or("completed");
                    p.exit_code = item
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .map(|c| c as i32);
                    p.status = safe_keyword(status);
                    let failed = matches!(status, "failed" | "declined")
                        || p.exit_code.is_some_and(|c| c != 0);
                    out.push((
                        if failed {
                            EventType::ToolFailed
                        } else {
                            EventType::ToolCompleted
                        },
                        p,
                    ));
                }
            }
            _ => {}
        }
        self.wrap(ctx, line, out)
    }

    fn wrap(
        &self,
        ctx: &NormalizeContext,
        line: &str,
        events: Vec<(EventType, EventPayload)>,
    ) -> Vec<EventEnvelope> {
        let Some(session) = self.thread_id.clone() else {
            return Vec::new();
        };
        events
            .into_iter()
            .map(|(t, p)| {
                let discriminator = format!("exec/{}", self.seq);
                envelope(
                    SOURCE,
                    ctx,
                    line.as_bytes(),
                    &discriminator,
                    session.clone(),
                    self.turn_id.clone(),
                    t,
                    None,
                    p,
                )
            })
            .collect()
    }
}
