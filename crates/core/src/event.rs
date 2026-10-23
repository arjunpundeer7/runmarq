//! Versioned, normalized event contract.
//!
//! Vendor adapters translate raw hook payloads into [`EventEnvelope`]s. The
//! envelope deliberately carries only timing and classification metadata:
//! prompt text, file contents, command lines and tool output never appear here.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SCHEMA_VERSION: u32 = 1;

/// Upper bound for a serialized envelope accepted by the collector.
pub const MAX_ENVELOPE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentSource {
    ClaudeCode,
    Codex,
    Cursor,
}

impl AgentSource {
    pub const ALL: [AgentSource; 3] = [
        AgentSource::ClaudeCode,
        AgentSource::Codex,
        AgentSource::Cursor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentSource::ClaudeCode => "claude_code",
            AgentSource::Codex => "codex",
            AgentSource::Cursor => "cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentSource::ClaudeCode => "Claude Code",
            AgentSource::Codex => "Codex",
            AgentSource::Cursor => "Cursor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "claude_code" | "claude" | "claudecode" => Some(AgentSource::ClaudeCode),
            "codex" | "codex_cli" => Some(AgentSource::Codex),
            "cursor" => Some(AgentSource::Cursor),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EventType {
    SessionStarted,
    SessionEnded,
    PromptSubmitted,
    RunStarted,
    ToolStarted,
    ToolCompleted,
    ToolFailed,
    PlanUpdated,
    SubagentStarted,
    SubagentCompleted,
    ApprovalRequested,
    ApprovalResolved,
    /// The agent reports it is idle and waiting for the user's next prompt.
    AgentIdle,
    ContextCompacting,
    RunCompleted,
    RunFailed,
    RunCancelled,
    Heartbeat,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::SessionStarted => "session_started",
            EventType::SessionEnded => "session_ended",
            EventType::PromptSubmitted => "prompt_submitted",
            EventType::RunStarted => "run_started",
            EventType::ToolStarted => "tool_started",
            EventType::ToolCompleted => "tool_completed",
            EventType::ToolFailed => "tool_failed",
            EventType::PlanUpdated => "plan_updated",
            EventType::SubagentStarted => "subagent_started",
            EventType::SubagentCompleted => "subagent_completed",
            EventType::ApprovalRequested => "approval_requested",
            EventType::ApprovalResolved => "approval_resolved",
            EventType::AgentIdle => "agent_idle",
            EventType::ContextCompacting => "context_compacting",
            EventType::RunCompleted => "run_completed",
            EventType::RunFailed => "run_failed",
            EventType::RunCancelled => "run_cancelled",
            EventType::Heartbeat => "heartbeat",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            EventType::RunCompleted | EventType::RunFailed | EventType::RunCancelled
        )
    }
}

/// Normalized classification of observable agent activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActivityCategory {
    ReadingFiles,
    EditingCode,
    SearchingRepository,
    RunningCommands,
    InstallingDependencies,
    Building,
    RunningTests,
    Debugging,
    WaitingForApproval,
    Research,
    Delegating,
    CompactingContext,
    Unknown,
}

impl ActivityCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityCategory::ReadingFiles => "reading_files",
            ActivityCategory::EditingCode => "editing_code",
            ActivityCategory::SearchingRepository => "searching_repository",
            ActivityCategory::RunningCommands => "running_commands",
            ActivityCategory::InstallingDependencies => "installing_dependencies",
            ActivityCategory::Building => "building",
            ActivityCategory::RunningTests => "running_tests",
            ActivityCategory::Debugging => "debugging",
            ActivityCategory::WaitingForApproval => "waiting_for_approval",
            ActivityCategory::Research => "research",
            ActivityCategory::Delegating => "delegating",
            ActivityCategory::CompactingContext => "compacting_context",
            ActivityCategory::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    pub fn label(self) -> &'static str {
        match self {
            ActivityCategory::ReadingFiles => "Reading files",
            ActivityCategory::EditingCode => "Editing code",
            ActivityCategory::SearchingRepository => "Searching repository",
            ActivityCategory::RunningCommands => "Running commands",
            ActivityCategory::InstallingDependencies => "Installing dependencies",
            ActivityCategory::Building => "Building",
            ActivityCategory::RunningTests => "Running tests",
            ActivityCategory::Debugging => "Debugging",
            ActivityCategory::WaitingForApproval => "Waiting for approval",
            ActivityCategory::Research => "Researching",
            ActivityCategory::Delegating => "Delegating to subagent",
            ActivityCategory::CompactingContext => "Compacting context",
            ActivityCategory::Unknown => "Working",
        }
    }
}

/// Task categories used for cold-start priors and historical cohorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskCategory {
    SmallEdit,
    BugFix,
    FeatureImplementation,
    UiDevelopment,
    Refactoring,
    Testing,
    DependencyWork,
    RepositoryAnalysis,
    Research,
    Migration,
    MultiStep,
    Unknown,
}

impl TaskCategory {
    pub const ALL: [TaskCategory; 12] = [
        TaskCategory::SmallEdit,
        TaskCategory::BugFix,
        TaskCategory::FeatureImplementation,
        TaskCategory::UiDevelopment,
        TaskCategory::Refactoring,
        TaskCategory::Testing,
        TaskCategory::DependencyWork,
        TaskCategory::RepositoryAnalysis,
        TaskCategory::Research,
        TaskCategory::Migration,
        TaskCategory::MultiStep,
        TaskCategory::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskCategory::SmallEdit => "small_edit",
            TaskCategory::BugFix => "bug_fix",
            TaskCategory::FeatureImplementation => "feature_implementation",
            TaskCategory::UiDevelopment => "ui_development",
            TaskCategory::Refactoring => "refactoring",
            TaskCategory::Testing => "testing",
            TaskCategory::DependencyWork => "dependency_work",
            TaskCategory::RepositoryAnalysis => "repository_analysis",
            TaskCategory::Research => "research",
            TaskCategory::Migration => "migration",
            TaskCategory::MultiStep => "multi_step",
            TaskCategory::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskCategory::SmallEdit => "Small edit",
            TaskCategory::BugFix => "Bug fix",
            TaskCategory::FeatureImplementation => "Feature",
            TaskCategory::UiDevelopment => "UI work",
            TaskCategory::Refactoring => "Refactor",
            TaskCategory::Testing => "Testing",
            TaskCategory::DependencyWork => "Dependencies",
            TaskCategory::RepositoryAnalysis => "Analysis",
            TaskCategory::Research => "Research",
            TaskCategory::Migration => "Migration",
            TaskCategory::MultiStep => "Multi-step",
            TaskCategory::Unknown => "General task",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ComplexityBucket {
    Trivial,
    Small,
    Medium,
    Large,
    VeryLarge,
}

impl ComplexityBucket {
    pub fn as_str(self) -> &'static str {
        match self {
            ComplexityBucket::Trivial => "trivial",
            ComplexityBucket::Small => "small",
            ComplexityBucket::Medium => "medium",
            ComplexityBucket::Large => "large",
            ComplexityBucket::VeryLarge => "very_large",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

/// Features derived locally from prompt text. The text itself is discarded
/// after classification; only these numbers (and an optional salted hash)
/// leave the hook process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PromptFeatures {
    pub category: TaskCategory,
    pub complexity: ComplexityBucket,
    /// Raw complexity score in [0, 1] before bucketing.
    pub complexity_score: f32,
    pub word_count: u32,
    pub list_items: u32,
    pub file_mentions: u32,
    pub mentions_tests: bool,
    /// Salted SHA-256 of the normalized prompt, used only to recognize repeats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fingerprint: Option<String>,
    /// Salted, hashed term vector used to find similar past prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sketch: Option<Vec<i8>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditStats {
    pub lines_added: u32,
    pub lines_removed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct PlanSnapshot {
    pub total_steps: u32,
    pub completed_steps: u32,
}

/// The agent process a hook ran under. The start time tells it apart from a
/// later process that reuses the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct AgentProcess {
    pub pid: u32,
    /// In the operating system's own units; only compared for equality.
    #[ts(type = "number")]
    pub started: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct RepositoryRef {
    /// Salted hash of the canonical repository root. Never a raw path.
    pub id: String,
    /// Folder name only, and only when the user allows repository names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TimestampOrigin {
    /// The vendor supplied the timestamp.
    Source,
    /// The vendor supplied none; this is when the hook process started.
    HookInvocation,
}

/// Typed, minimized event payload. Every field is optional because adapters
/// differ in what they can observe. Unknown fields are dropped, so a newer
/// hook can talk to an older collector.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EventPayload {
    /// Tool identifier such as `Bash` or `Edit`. Never arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub activity: Option<ActivityCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub duration_ms: Option<u64>,
    /// Vendor status keyword, for example `completed`, `aborted`, `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<String>,
    /// Short machine-readable reason, for example `permission_prompt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub is_interrupt: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repository: Option<RepositoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<PromptFeatures>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan: Option<PlanSnapshot>,
    /// One task in a task list the agent edits a task at a time, with its new
    /// state in `status`. The collector turns these into a `plan`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub subagent_id: Option<String>,
    /// Short task description. Present only when the user opted in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    /// Set when the agent was started through `runmarq run`, so the wrapper
    /// can report process exit for the sessions it launched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub launcher_id: Option<String>,
    /// Lets the collector end a run whose agent was closed without saying so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_process: Option<AgentProcess>,
    /// Reasoning effort: low, medium, high, xhigh or max.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<String>,
    /// Salted hash of a normalized shell command, such as the one for `npm test`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub edit: Option<EditStats>,
    /// Agent session transcript. Read locally for token and turn counts; not stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub transcript_path: Option<String>,
    /// Agent working directory. Used for git diff sampling; not stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
    /// The prompt came from the agent itself, such as a background task
    /// reporting back, rather than from the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub injected: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub event_id: String,
    pub source: AgentSource,
    pub source_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_turn_id: Option<String>,
    #[ts(type = "string")]
    pub timestamp: DateTime<Utc>,
    pub timestamp_origin: TimestampOrigin,
    #[serde(rename = "type")]
    pub event_type: EventType,
    #[serde(default)]
    pub payload: EventPayload,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    #[error("unsupported schema version {0}")]
    SchemaVersion(u32),
    #[error("field `{field}` is invalid: {reason}")]
    Field {
        field: &'static str,
        reason: &'static str,
    },
}

fn check_id(field: &'static str, v: &str, max: usize) -> Result<(), ValidationError> {
    if v.is_empty() {
        return Err(ValidationError::Field {
            field,
            reason: "empty",
        });
    }
    if v.len() > max {
        return Err(ValidationError::Field {
            field,
            reason: "too long",
        });
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.:/@".contains(c))
    {
        return Err(ValidationError::Field {
            field,
            reason: "unexpected characters",
        });
    }
    Ok(())
}

fn check_text(field: &'static str, v: &str, max: usize) -> Result<(), ValidationError> {
    if v.chars().count() > max {
        return Err(ValidationError::Field {
            field,
            reason: "too long",
        });
    }
    if v.chars().any(|c| c.is_control()) {
        return Err(ValidationError::Field {
            field,
            reason: "control characters",
        });
    }
    Ok(())
}

impl EventEnvelope {
    /// Semantic validation applied by the collector to every inbound event.
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ValidationError::SchemaVersion(self.schema_version));
        }
        check_id("eventId", &self.event_id, 128)?;
        check_id("sourceSessionId", &self.source_session_id, 200)?;
        if let Some(t) = &self.source_turn_id {
            check_id("sourceTurnId", t, 200)?;
        }
        if self.timestamp > now + Duration::hours(24) {
            return Err(ValidationError::Field {
                field: "timestamp",
                reason: "too far in the future",
            });
        }
        let p = &self.payload;
        if let Some(v) = &p.tool_name {
            check_text("toolName", v, 128)?;
        }
        if let Some(v) = &p.tool_use_id {
            check_id("toolUseId", v, 200)?;
        }
        if let Some(v) = &p.status {
            check_id("status", v, 64)?;
        }
        if let Some(v) = &p.reason {
            check_id("reason", v, 64)?;
        }
        if let Some(v) = &p.model {
            check_text("model", v, 128)?;
        }
        if let Some(v) = &p.agent_version {
            check_text("agentVersion", v, 64)?;
        }
        if let Some(v) = &p.task_id {
            check_id("taskId", v, 64)?;
        }
        if let Some(v) = &p.subagent_id {
            check_id("subagentId", v, 200)?;
        }
        if let Some(v) = &p.title {
            check_text("title", v, 160)?;
        }
        if let Some(v) = &p.launcher_id {
            check_id("launcherId", v, 64)?;
        }
        if let Some(v) = &p.effort {
            check_id("effort", v, 24)?;
        }
        if let Some(v) = &p.permission_mode {
            check_id("permissionMode", v, 32)?;
        }
        if let Some(v) = &p.command_key {
            check_id("commandKey", v, 48)?;
        }
        if let Some(v) = &p.transcript_path {
            check_text("transcriptPath", v, 1024)?;
        }
        if let Some(v) = &p.cwd {
            check_text("cwd", v, 1024)?;
        }
        if let Some(e) = &p.edit {
            if e.lines_added > 1_000_000 || e.lines_removed > 1_000_000 {
                return Err(ValidationError::Field {
                    field: "edit",
                    reason: "out of range",
                });
            }
        }
        if let Some(r) = &p.repository {
            check_id("repository.id", &r.id, 64)?;
            if let Some(n) = &r.display_name {
                check_text("repository.displayName", n, 128)?;
            }
        }
        if let Some(plan) = &p.plan {
            if plan.completed_steps > plan.total_steps || plan.total_steps > 500 {
                return Err(ValidationError::Field {
                    field: "plan",
                    reason: "inconsistent step counts",
                });
            }
        }
        if let Some(f) = &p.prompt {
            if !(0.0..=1.0).contains(&f.complexity_score) || f.complexity_score.is_nan() {
                return Err(ValidationError::Field {
                    field: "prompt.complexityScore",
                    reason: "out of range",
                });
            }
            if let Some(fp) = &f.fingerprint {
                check_id("prompt.fingerprint", fp, 64)?;
            }
            if f.sketch.as_ref().is_some_and(|v| v.len() > 512) {
                return Err(ValidationError::Field {
                    field: "prompt.sketch",
                    reason: "too long",
                });
            }
        }
        Ok(())
    }
}

/// Deterministic event identifier derived from the source, the raw vendor
/// payload, and a coarse time bucket. Identical payloads delivered twice in
/// quick succession (a known failure mode of some hook runners) collapse to
/// one event, while genuinely repeated actions minutes apart stay distinct.
pub fn derive_event_id(
    source: AgentSource,
    discriminator: &str,
    raw: &[u8],
    at: DateTime<Utc>,
) -> String {
    use sha2::{Digest, Sha256};
    let bucket = at.timestamp() / 2;
    let mut h = Sha256::new();
    h.update(source.as_str().as_bytes());
    h.update([0]);
    h.update(discriminator.as_bytes());
    h.update([0]);
    h.update(raw);
    h.update([0]);
    h.update(bucket.to_le_bytes());
    let digest = h.finalize();
    format!("ev_{}", hex::encode(&digest[..16]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EventEnvelope {
        EventEnvelope {
            schema_version: SCHEMA_VERSION,
            event_id: "ev_1".into(),
            source: AgentSource::ClaudeCode,
            source_session_id: "abc-123".into(),
            source_turn_id: None,
            timestamp: Utc::now(),
            timestamp_origin: TimestampOrigin::HookInvocation,
            event_type: EventType::ToolStarted,
            payload: EventPayload {
                tool_name: Some("Bash".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn roundtrip_and_validate() {
        let e = sample();
        let json = serde_json::to_string(&e).unwrap();
        let back: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
        back.validate(Utc::now()).unwrap();
        assert!(json.contains("\"type\":\"tool_started\""));
    }

    #[test]
    fn unknown_payload_fields_are_dropped_and_envelope_fields_rejected() {
        let mut v = serde_json::to_value(sample()).unwrap();
        v["payload"]["promptText"] = serde_json::json!("secret");
        let parsed: EventEnvelope = serde_json::from_value(v.clone()).unwrap();
        assert!(!serde_json::to_string(&parsed).unwrap().contains("secret"));
        v["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<EventEnvelope>(v).is_err());
    }

    #[test]
    fn rejects_bad_ids_and_versions() {
        let mut e = sample();
        e.source_session_id = "a b; rm -rf".into();
        assert!(e.validate(Utc::now()).is_err());
        let mut e = sample();
        e.schema_version = 99;
        assert_eq!(
            e.validate(Utc::now()),
            Err(ValidationError::SchemaVersion(99))
        );
        let mut e = sample();
        e.timestamp = Utc::now() + Duration::days(3);
        assert!(e.validate(Utc::now()).is_err());
    }

    #[test]
    fn event_ids_are_stable_within_bucket() {
        let t = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let a = derive_event_id(AgentSource::Cursor, "stop", b"{}", t);
        let b = derive_event_id(
            AgentSource::Cursor,
            "stop",
            b"{}",
            t + Duration::milliseconds(900),
        );
        let c = derive_event_id(
            AgentSource::Cursor,
            "stop",
            b"{}",
            t + Duration::seconds(10),
        );
        let d = derive_event_id(AgentSource::ClaudeCode, "stop", b"{}", t);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn enum_string_forms_match_serde() {
        for c in TaskCategory::ALL {
            assert_eq!(TaskCategory::parse(c.as_str()), Some(c));
        }
        assert_eq!(
            EventType::parse("run_completed"),
            Some(EventType::RunCompleted)
        );
        assert_eq!(AgentSource::parse("claude"), Some(AgentSource::ClaudeCode));
    }
}
