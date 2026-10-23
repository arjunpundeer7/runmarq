//! Data transfer objects served by the local collector API and consumed by the
//! CLI and the desktop app.

use crate::capabilities::{AgentCapabilities, SupportLevel};
use crate::event::{ActivityCategory, AgentSource, EventType, PlanSnapshot, TaskCategory};
use crate::forecast::EtaForecast;
use crate::run::RunState;
use crate::signals::{Phase, RunSignals};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AttentionKind {
    /// The agent is asking for permission or input.
    ApprovalRequired,
    /// No events for a long time; state cannot be determined.
    ConnectionLost,
    /// Clear signs that the run is not making progress.
    Stuck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunView {
    pub id: String,
    pub session_id: String,
    pub agent: AgentSource,
    pub repository_id: Option<String>,
    pub repository_name: Option<String>,
    pub title: Option<String>,
    pub task_category: TaskCategory,
    pub prompt_known: bool,
    pub model: Option<String>,
    pub state: RunState,
    pub exit_reason: Option<String>,
    #[ts(type = "string")]
    pub started_at: DateTime<Utc>,
    #[ts(type = "string | null")]
    pub ended_at: Option<DateTime<Utc>>,
    #[ts(type = "string")]
    pub last_event_at: DateTime<Utc>,
    #[ts(type = "number")]
    pub wall_ms: u64,
    #[ts(type = "number")]
    pub active_ms: u64,
    #[ts(type = "number")]
    pub waiting_ms: u64,
    pub current_activity: Option<ActivityCategory>,
    pub current_tool: Option<String>,
    pub attention: Option<AttentionKind>,
    /// Short reason for the attention state, for example why a run looks stuck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attention_detail: Option<String>,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub plan: Option<PlanSnapshot>,
    pub forecast: Option<EtaForecast>,
    /// Remaining-time range predicted at the start of the run, for comparison
    /// with the actual duration in history views.
    pub initial_p10_seconds: Option<u32>,
    pub initial_p50_seconds: Option<u32>,
    pub initial_p90_seconds: Option<u32>,
    pub effort: Option<String>,
    pub phase: Option<Phase>,
    pub signals: Option<RunSignals>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EventView {
    pub id: String,
    pub event_type: EventType,
    #[ts(type = "string")]
    pub source_timestamp: DateTime<Utc>,
    #[ts(type = "string")]
    pub received_timestamp: DateTime<Utc>,
    pub activity: Option<ActivityCategory>,
    pub tool: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhaseSpan {
    pub activity: ActivityCategory,
    #[ts(type = "string")]
    pub started_at: DateTime<Utc>,
    #[ts(type = "string | null")]
    pub ended_at: Option<DateTime<Utc>>,
    pub tool_calls: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForecastChange {
    #[ts(type = "string")]
    pub at: DateTime<Utc>,
    /// Human-readable, factual explanation, e.g. "Estimate increased after repeated test failures."
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunDetail {
    pub run: RunView,
    pub events: Vec<EventView>,
    pub forecasts: Vec<EtaForecast>,
    pub phases: Vec<PhaseSpan>,
    pub changes: Vec<ForecastChange>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryQuery {
    #[serde(default)]
    #[ts(optional)]
    pub agent: Option<AgentSource>,
    #[serde(default)]
    #[ts(optional)]
    pub repository_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub state: Option<RunState>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub to: Option<DateTime<Utc>>,
    #[serde(default)]
    #[ts(optional)]
    pub search: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryPage {
    pub runs: Vec<RunView>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepositoryView {
    pub id: String,
    pub display_name: Option<String>,
    pub run_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DurationBucket {
    pub key: String,
    pub label: String,
    pub runs: u32,
    #[ts(type = "number | null")]
    pub median_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub p90_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ErrorPoint {
    pub label: String,
    pub runs: u32,
    pub median_abs_error_seconds: Option<f64>,
    pub coverage: Option<f64>,
}

/// Accuracy report. Every metric is `None` when there is not enough data to
/// state it honestly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccuracyReport {
    pub evaluated_runs: u32,
    pub minimum_runs_for_metrics: u32,
    pub median_abs_error_seconds: Option<f64>,
    pub mean_abs_error_seconds: Option<f64>,
    pub median_abs_pct_error: Option<f64>,
    /// Fraction of evaluated forecasts whose P10–P90 interval contained the outcome.
    pub interval_coverage: Option<f64>,
    pub insufficient_info_fraction: Option<f64>,
    /// Mean relative change of P50 between consecutive forecasts.
    pub stability: Option<f64>,
    pub by_forecast_age: Vec<ErrorPoint>,
    pub by_agent: Vec<ErrorPoint>,
    pub by_category: Vec<ErrorPoint>,
    /// Temporal holdout: model fitted on earlier runs, evaluated on later ones.
    pub holdout: Option<HoldoutReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HoldoutReport {
    pub train_runs: u32,
    pub test_runs: u32,
    pub median_abs_error_seconds: f64,
    pub interval_coverage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Insights {
    pub total_runs: u32,
    pub finished_runs: u32,
    #[ts(type = "number | null")]
    pub median_duration_ms: Option<u64>,
    #[ts(type = "number")]
    pub total_waiting_ms: u64,
    pub by_agent: Vec<DurationBucket>,
    pub by_category: Vec<DurationBucket>,
    pub accuracy: AccuracyReport,
    pub model: ModelStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelComponent {
    pub key: String,
    pub label: String,
    pub active: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CalibrationStatus {
    pub active: bool,
    /// Conformity scores from forecasts of runs the models had not seen.
    pub scores: u32,
    /// Finished runs those forecasts came from.
    pub validation_runs: u32,
    /// Finished runs needed before calibration can start.
    pub required_runs: u32,
    pub target_coverage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelStatus {
    pub version: String,
    #[ts(type = "string | null")]
    pub trained_at: Option<DateTime<Utc>>,
    pub finished_runs: u32,
    pub components: Vec<ModelComponent>,
    pub calibration: CalibrationStatus,
}

/// Counts the agents wrote to their local transcripts, summed over runs.
/// This is not the vendor's usage meter: input and cached tokens are not
/// included, and plan limits are not known to runmarq.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageTotals {
    /// Runs with any activity in the period.
    pub runs: u32,
    /// Model responses, subagents included.
    pub responses: u32,
    /// Output tokens, thinking included.
    #[ts(type = "number")]
    pub output_tokens: u64,
    /// The part of `output_tokens` spent thinking, where the agent reports it.
    #[ts(type = "number")]
    pub thinking_tokens: u64,
    /// Agent working time, excluding time spent waiting for you.
    #[ts(type = "number")]
    pub active_ms: u64,
}

/// A five-hour usage window. It opens with the first activity after the
/// previous window closed, the way Claude plans meter usage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageWindow {
    #[ts(type = "string")]
    pub started_at: DateTime<Utc>,
    #[ts(type = "string")]
    pub ends_at: DateTime<Utc>,
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentUsage {
    pub agent: AgentSource,
    /// Whether this agent's transcripts carry token counts at all.
    pub reports_tokens: bool,
    pub today: UsageTotals,
    pub last_seven_days: UsageTotals,
    /// The window open now, if the agent was used recently.
    pub current_window: Option<UsageWindow>,
    /// Closed windows from the last seven days, newest first.
    pub recent_windows: Vec<UsageWindow>,
    /// The user's own output-token budget per window, if they set one.
    #[ts(type = "number | null")]
    pub window_budget_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageReport {
    #[ts(type = "string")]
    pub generated_at: DateTime<Utc>,
    /// Local midnight, where "today" starts.
    #[ts(type = "string")]
    pub today_started_at: DateTime<Utc>,
    pub window_hours: u32,
    /// Token counts come from transcripts, which can be turned off.
    pub transcripts_enabled: bool,
    /// Agents with runs in the last seven days.
    pub agents: Vec<AgentUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IntegrationState {
    /// The agent is not installed on this machine.
    NotDetected,
    /// Installed, but no runmarq integration is configured.
    NotConnected,
    /// Configuration written and valid, but no event has been received from
    /// the agent yet, so the end-to-end path is not yet verified.
    AwaitingFirstEvent,
    /// Integration configured and verified by a received event.
    Connected,
    /// Configured, but with documented limitations or a pending manual step.
    Limited,
    /// Configuration exists but verification failed.
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IntegrationCheck {
    pub label: String,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IntegrationStatus {
    pub agent: AgentSource,
    pub display_name: String,
    pub detected: bool,
    pub installed_version: Option<String>,
    pub executable: Option<String>,
    pub state: IntegrationState,
    pub support: SupportLevel,
    pub capabilities: AgentCapabilities,
    pub limitations: Vec<String>,
    /// Files runmarq changes on setup (shown to the user before approval).
    pub config_locations: Vec<String>,
    pub checks: Vec<IntegrationCheck>,
    /// Manual steps the user must still complete in the agent itself.
    pub pending_steps: Vec<String>,
    #[ts(type = "string | null")]
    pub last_event_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LatencyStats {
    pub samples: u32,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Diagnostics {
    pub collector_version: String,
    pub schema_version: u32,
    pub pid: u32,
    #[ts(type = "string")]
    pub started_at: DateTime<Utc>,
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    pub data_dir: String,
    #[ts(type = "number")]
    pub database_bytes: u64,
    #[ts(type = "number")]
    pub events_received: u64,
    #[ts(type = "number")]
    pub events_rejected: u64,
    #[ts(type = "number")]
    pub events_duplicate: u64,
    #[ts(type = "number")]
    pub spooled_events_replayed: u64,
    #[ts(type = "number")]
    pub spool_pending: u64,
    pub subscribers: u32,
    pub open_runs: u32,
    pub ingest_latency: LatencyStats,
    pub forecast_latency: LatencyStats,
    pub db_write_latency: LatencyStats,
    /// Latency reported by `runmarq emit` for the full hook path.
    pub hook_latency: LatencyStats,
    /// Time taken to refit the estimation models.
    #[serde(default)]
    pub training_latency: LatencyStats,
    pub recent_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Health {
    pub ok: bool,
    pub version: String,
    pub schema_version: u32,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Preferences {
    /// Store a short task description derived from the prompt (first line, truncated).
    pub store_task_titles: bool,
    /// Store repository folder names (never full paths).
    pub store_repository_names: bool,
    pub notifications_enabled: bool,
    pub notify_on_finish: bool,
    pub notify_on_failure: bool,
    pub notify_on_attention: bool,
    pub notify_on_disconnect: bool,
    /// Notify once when a run shows clear signs of being stuck.
    pub notify_on_stuck: bool,
    /// Play the system notification sound.
    pub notification_sound: bool,
    /// Suppress all notifications without changing the individual toggles.
    pub quiet_mode: bool,
    /// Only notify about runs that took at least this long.
    pub notify_min_duration_seconds: u32,
    /// Delete run history older than this many days. `None` keeps everything.
    pub retention_days: Option<u32>,
    pub compact_widget: bool,
    pub widget_always_on_top: bool,
    pub onboarding_completed: bool,
    /// Read agent transcripts for token, turn and context counts.
    pub read_transcripts: bool,
    /// Sample `git diff --shortstat` during runs to measure code change.
    pub sample_git_diff: bool,
    /// Keep a hashed term vector of each prompt to find similar past runs.
    pub similar_prompts: bool,
    /// The user's own output-token budgets per five-hour window, by agent,
    /// shown as progress on the usage page. Agents without one show counts only.
    #[ts(as = "HashMap<AgentSource, f64>")]
    pub usage_window_budgets: HashMap<AgentSource, u64>,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            store_task_titles: false,
            store_repository_names: true,
            notifications_enabled: true,
            notify_on_finish: true,
            notify_on_failure: true,
            notify_on_attention: true,
            notify_on_disconnect: true,
            notify_on_stuck: true,
            notification_sound: true,
            quiet_mode: false,
            notify_min_duration_seconds: 20,
            retention_days: None,
            compact_widget: false,
            widget_always_on_top: false,
            onboarding_completed: false,
            read_transcripts: true,
            sample_git_diff: true,
            similar_prompts: true,
            usage_window_budgets: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotificationKind {
    RunFinished,
    RunFailed,
    AttentionRequired,
    ConnectionLost,
    Stuck,
}

impl NotificationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationKind::RunFinished => "run_finished",
            NotificationKind::RunFailed => "run_failed",
            NotificationKind::AttentionRequired => "attention_required",
            NotificationKind::ConnectionLost => "connection_lost",
            NotificationKind::Stuck => "stuck",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationRequest {
    pub id: String,
    pub run_id: String,
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
}

/// Messages pushed to subscribers over the event stream.
// Snapshot and update variants are large by nature; they are serialized once per send.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum StreamMessage {
    Snapshot { runs: Vec<RunView> },
    RunUpdated { run: RunView },
    Notification { notification: NotificationRequest },
    PreferencesChanged { preferences: Preferences },
}

/// Hook-side snapshot of the preferences `runmarq emit` needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HookConfig {
    pub store_task_titles: bool,
    pub store_repository_names: bool,
    pub prompt_sketches: bool,
}

impl HookConfig {
    pub fn from_preferences(p: &Preferences) -> HookConfig {
        HookConfig {
            store_task_titles: p.store_task_titles,
            store_repository_names: p.store_repository_names,
            prompt_sketches: p.similar_prompts,
        }
    }
}

/// Contents of the discovery file written by a running collector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub version: String,
    pub started_at: DateTime<Utc>,
}

/// One hook invocation's self-measured latency, reported to the collector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IngestRequest {
    pub events: Vec<crate::event::EventEnvelope>,
    #[serde(default)]
    pub hook_latency_ms: Option<f64>,
    #[serde(default)]
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestResponse {
    pub accepted: u32,
    pub duplicates: u32,
    pub rejected: u32,
}
