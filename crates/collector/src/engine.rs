//! Run state engine.
//!
//! Synchronous and deterministic given its inputs, so it is tested directly
//! without the HTTP layer. The server wraps it in a mutex and runs it on a
//! blocking thread; file and process work (transcripts, `git`, model
//! training) is handed out as requests and done outside the lock.
//!
//! Routing rules:
//! - Sessions are keyed by (source, vendor session id), so identical vendor
//!   ids from different agents never collide.
//! - With a vendor turn id (Codex, Cursor), events join the run with that turn.
//! - Without one (Claude Code), events join the session's open run, or the
//!   historical run whose time window contains them when they arrive late.
//! - A prompt that arrives while the agent is still busy continues the open
//!   run. Claude Code submits background-task notifications and messages
//!   typed during a turn this way, at a tool boundary, and the agent keeps
//!   working: one task, one run.
//! - Any other new prompt while a run is still open closes that run as
//!   `cancelled` with reason `superseded`, ending at its last activity: the
//!   agent went quiet without reporting an end, which is what an interrupt
//!   looks like. It is never marked completed.

use crate::learning;
use crate::probe::{self, GitProbe, ProbeRequest, ProbeResult, TranscriptCursor};
use crate::stats::Samples;
use crate::stuck;
use crate::tracker::{to_view, Tracker};
use chrono::{DateTime, Duration, Utc};
use runmarq_adapters::transcript::TranscriptFormat;
use runmarq_core::api::{
    AttentionKind, DurationBucket, EventView, ForecastChange, HistoryPage, HistoryQuery,
    HookConfig, Insights, NotificationKind, NotificationRequest, PhaseSpan, Preferences, RunDetail,
    RunView, StreamMessage,
};
use runmarq_core::event::{
    ActivityCategory, AgentProcess, AgentSource, EventEnvelope, EventType, TaskCategory,
};
use runmarq_core::run::{ExitReason, RunState};
use runmarq_core::signals::RunSignals;
use runmarq_estimator::command::CommandStat;
use runmarq_estimator::eval::{evaluate, EvalRun};
use runmarq_estimator::{change_message, smooth, Estimator, ForecastInput};
use runmarq_storage::{CommandStatRow, EventRow, RunRow, SessionRow, Store, StoreError};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

/// No events for this long while working: state becomes `unknown`.
pub const STALE_AFTER: Duration = Duration::minutes(20);
/// Unknown runs are dropped from memory (they stay `unknown` on disk).
pub const EVICT_UNKNOWN_AFTER: Duration = Duration::hours(6);
/// Minimum interval between periodic forecast refreshes.
pub const FORECAST_REFRESH: Duration = Duration::seconds(15);
/// Persist a forecast at least this often even if unchanged.
pub const FORECAST_PERSIST_EVERY: Duration = Duration::seconds(60);
/// Models are refitted after this many runs finish, or this long after the last fit.
pub const REFIT_AFTER_RUNS: u32 = 3;
pub const REFIT_INTERVAL: Duration = Duration::minutes(10);
pub const HISTORY_LIMIT: u32 = 3000;
const GIT_PROBE_EVERY: Duration = Duration::seconds(30);
const FILE_COUNT_EVERY: Duration = Duration::hours(24);
/// Signal snapshots for training are kept at most this often, in active time.
const SIGNAL_SNAPSHOT_EVERY_MS: u64 = 20_000;
const MAX_TOOL_STARTS: usize = 2000;
/// A user prompt this soon after the agent's last event continues its run.
const CONTINUE_WITHIN: Duration = Duration::seconds(10);
/// Tool calls in flight for longer than this are treated as lost.
const TOOL_IN_FLIGHT_MAX: Duration = Duration::minutes(15);
/// A run's agent process is checked only once it has been quiet this long,
/// so events sent just before the agent exited have arrived.
const PROCESS_QUIET: Duration = Duration::seconds(5);
const RECENT_IDS: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid event: {0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Clone, Default)]
struct SessionInfo {
    id: String,
    open_run: Option<String>,
    model: Option<String>,
    launcher_id: Option<String>,
    process: Option<AgentProcess>,
}

#[derive(Debug, Default)]
pub struct EngineStats {
    pub received: u64,
    pub rejected: u64,
    pub duplicates: u64,
    pub replayed: u64,
    pub ingest_latency: Samples,
    pub forecast_latency: Samples,
    pub db_latency: Samples,
    pub hook_latency: Samples,
    pub training_ms: Samples,
    pub recent_errors: VecDeque<String>,
}

impl EngineStats {
    pub fn error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::warn!("{msg}");
        self.recent_errors
            .push_back(format!("{} {msg}", Utc::now().format("%H:%M:%S")));
        while self.recent_errors.len() > 20 {
            self.recent_errors.pop_front();
        }
    }
}

#[derive(Debug, Default)]
pub struct IngestOutcome {
    pub duplicate: bool,
    pub messages: Vec<StreamMessage>,
}

/// Where model fitting happens. `Inline` fits on demand inside the engine
/// (tests, tools); `Background` leaves it to the server, off the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingMode {
    Inline,
    Background,
}

#[derive(Debug)]
struct Training {
    mode: TrainingMode,
    /// Bumped whenever the training data changes.
    generation: u64,
    fitted_generation: Option<u64>,
    running: bool,
    last_fit: Option<DateTime<Utc>>,
    finished_since_fit: u32,
    urgent: bool,
}

impl Training {
    fn dirty(&self) -> bool {
        self.fitted_generation != Some(self.generation)
    }
}

pub struct Engine {
    store: Store,
    estimator: Estimator,
    training: Training,
    trackers: HashMap<String, Tracker>,
    sessions: HashMap<(AgentSource, String), SessionInfo>,
    prefs: Preferences,
    recent_ids: VecDeque<String>,
    recent_set: HashSet<String>,
    repo_names: HashMap<String, Option<String>>,
    repo_files: HashMap<String, u32>,
    files_counted: HashMap<String, DateTime<Utc>>,
    commands: HashMap<(String, String), CommandStat>,
    measured_coverage: Option<f64>,
    finished_since_eval: u32,
    hook_config_path: Option<PathBuf>,
    process_running: fn(&AgentProcess) -> bool,
    pub stats: EngineStats,
}

fn human_duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => "under a minute".into(),
        60..=3599 => format!("{} min", (s + 30) / 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

impl Engine {
    pub fn new(store: Store) -> Result<Engine> {
        let prefs = store.load_preferences()?;
        let commands = store
            .command_stats()?
            .iter()
            .map(|r| {
                (
                    (r.repository_id.clone(), r.command_key.clone()),
                    learning::command_stat(r),
                )
            })
            .collect();
        let repo_files = store.repository_file_counts()?;
        let mut e = Engine {
            store,
            estimator: Estimator::default(),
            training: Training {
                mode: TrainingMode::Inline,
                generation: 0,
                fitted_generation: None,
                running: false,
                last_fit: None,
                finished_since_fit: 0,
                urgent: true,
            },
            trackers: HashMap::new(),
            sessions: HashMap::new(),
            prefs,
            recent_ids: VecDeque::new(),
            recent_set: HashSet::new(),
            repo_names: HashMap::new(),
            repo_files,
            files_counted: HashMap::new(),
            commands,
            measured_coverage: None,
            finished_since_eval: 0,
            hook_config_path: None,
            process_running: crate::process::is_running,
            stats: EngineStats::default(),
        };
        e.refresh_calibration();
        Ok(e)
    }

    /// Replaces the check for whether an agent process is still running.
    pub fn set_process_check(&mut self, check: fn(&AgentProcess) -> bool) {
        self.process_running = check;
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    pub fn estimator(&self) -> &Estimator {
        &self.estimator
    }

    pub fn set_training_mode(&mut self, mode: TrainingMode) {
        self.training.mode = mode;
    }

    pub fn set_hook_config_path(&mut self, p: PathBuf) {
        self.hook_config_path = Some(p);
        self.write_hook_config();
    }

    fn write_hook_config(&mut self) {
        let Some(p) = &self.hook_config_path else {
            return;
        };
        let cfg = HookConfig::from_preferences(&self.prefs);
        let tmp = p.with_extension("json.tmp");
        let res = serde_json::to_vec(&cfg)
            .map_err(std::io::Error::other)
            .and_then(|b| std::fs::write(&tmp, b))
            .and_then(|_| std::fs::rename(&tmp, p));
        if let Err(e) = res {
            self.stats
                .error(format!("could not write hook config: {e}"));
        }
    }

    pub fn preferences(&self) -> &Preferences {
        &self.prefs
    }

    pub fn open_run_count(&self) -> usize {
        self.trackers.len()
    }

    // ------------------------------------------------------------ training

    fn training_changed(&mut self) {
        self.training.generation += 1;
    }

    /// Whether the server should start a background fit now.
    pub fn training_due(&self, now: DateTime<Utc>) -> bool {
        let t = &self.training;
        t.mode == TrainingMode::Background
            && !t.running
            && t.dirty()
            && (t.urgent
                || t.last_fit.is_none()
                || t.finished_since_fit >= REFIT_AFTER_RUNS
                || t.last_fit.is_some_and(|l| now - l >= REFIT_INTERVAL))
    }

    /// Marks a background fit as started; returns the data generation it covers.
    pub fn begin_training(&mut self) -> u64 {
        self.training.running = true;
        self.training.urgent = false;
        self.training.finished_since_fit = 0;
        self.training.generation
    }

    pub fn install_estimator(
        &mut self,
        mut estimator: Estimator,
        generation: u64,
        now: DateTime<Utc>,
    ) {
        // Commands timed while the fit ran are in memory but not in its data.
        estimator.set_commands(self.commands.values().cloned());
        self.estimator = estimator;
        self.training.running = false;
        self.training.fitted_generation = Some(generation);
        self.training.last_fit = Some(now);
    }

    pub fn training_failed(&mut self, msg: &str) {
        self.training.running = false;
        self.stats.error(format!("model training failed: {msg}"));
    }

    /// Fits the models in place when running inline and the data changed.
    pub fn ensure_trained(&mut self) {
        if self.training.mode != TrainingMode::Inline || !self.training.dirty() {
            return;
        }
        let generation = self.training.generation;
        let started = Instant::now();
        match learning::training_set(&self.store, HISTORY_LIMIT) {
            Ok(set) => {
                let est = Estimator::fit(&set, Utc::now());
                self.stats
                    .training_ms
                    .push(started.elapsed().as_secs_f64() * 1000.0);
                self.install_estimator(est, generation, Utc::now());
            }
            Err(e) => self.training_failed(&e.to_string()),
        }
    }

    // ----------------------------------------------------------- lifecycle

    /// Restores open runs after a restart. Their true state is unknown until
    /// the agent sends another event, so they are shown as such.
    pub fn recover(&mut self, now: DateTime<Utc>) -> Result<Vec<StreamMessage>> {
        let mut msgs = Vec::new();
        for row in self.store.open_runs()? {
            let mut t = Tracker::new(row);
            if t.row.status != RunState::Unknown {
                t.row.status = RunState::Unknown;
                t.row.exit_reason = Some(ExitReason::CollectorRestarted.as_str().into());
                t.attention = Some(AttentionKind::ConnectionLost);
            }
            if now - t.row.last_event_at > EVICT_UNKNOWN_AFTER {
                t.sync_row(now);
                self.store.upsert_run(&t.row)?;
                continue;
            }
            let events = self.store.events_for_run(&t.row.id)?;
            t.timeline = learning::timeline_from_rows(t.row.started_at, &events);
            t.sync_row(now);
            self.store.upsert_run(&t.row)?;
            if let Some(s) = self.session_by_id(&t.row.session_id)? {
                let key = (s.agent, s.source_session_id.clone());
                let info = self.sessions.entry(key).or_insert_with(|| SessionInfo {
                    id: s.id.clone(),
                    ..Default::default()
                });
                info.open_run = Some(t.row.id.clone());
            }
            let id = t.row.id.clone();
            self.trackers.insert(id.clone(), t);
            msgs.push(self.run_message(&id, now));
        }
        Ok(msgs)
    }

    fn session_by_id(&self, id: &str) -> Result<Option<SessionRow>> {
        Ok(self.store.get_session(id)?)
    }

    // -------------------------------------------------------------- ingest

    fn seen(&mut self, id: &str) -> Result<bool> {
        if self.recent_set.contains(id) || self.store.event_exists(id)? {
            return Ok(true);
        }
        self.recent_set.insert(id.to_string());
        self.recent_ids.push_back(id.to_string());
        while self.recent_ids.len() > RECENT_IDS {
            if let Some(old) = self.recent_ids.pop_front() {
                self.recent_set.remove(&old);
            }
        }
        Ok(false)
    }

    pub fn ingest(&mut self, env: EventEnvelope, received: DateTime<Utc>) -> Result<IngestOutcome> {
        let started = Instant::now();
        self.stats.received += 1;
        if let Err(e) = env.validate(received) {
            self.stats.rejected += 1;
            self.stats
                .error(format!("rejected event from {}: {e}", env.source));
            return Err(EngineError::Invalid(e.to_string()));
        }
        if self.seen(&env.event_id)? {
            self.stats.duplicates += 1;
            return Ok(IngestOutcome {
                duplicate: true,
                messages: vec![],
            });
        }

        self.store.begin()?;
        let result = self.ingest_inner(&env, received);
        match &result {
            Ok(_) => self.store.commit()?,
            Err(_) => self.store.rollback(),
        }
        self.stats
            .ingest_latency
            .push(started.elapsed().as_secs_f64() * 1000.0);
        result
    }

    fn ingest_inner(
        &mut self,
        env: &EventEnvelope,
        received: DateTime<Utc>,
    ) -> Result<IngestOutcome> {
        // Clock skew guard: never let an event appear to come from the future.
        let ts = env.timestamp.min(received + Duration::seconds(5));
        let key = (env.source, env.source_session_id.clone());
        let repo_id = env.payload.repository.as_ref().map(|r| r.id.clone());
        if let Some(repo) = &env.payload.repository {
            let name = if self.prefs.store_repository_names {
                repo.display_name.as_deref()
            } else {
                None
            };
            self.store.upsert_repository(&repo.id, name, ts)?;
            if name.is_some() {
                self.repo_names
                    .insert(repo.id.clone(), name.map(String::from));
            }
        }
        let session_id = self.ensure_session(&key, ts, repo_id.as_deref())?;
        if let Some(l) = &env.payload.launcher_id {
            if let Some(info) = self.sessions.get_mut(&key) {
                info.launcher_id = Some(l.clone());
            }
        }
        if let Some(p) = env.payload.agent_process {
            if let Some(info) = self.sessions.get_mut(&key) {
                info.process = Some(p);
            }
        }
        if env.event_type == EventType::SessionStarted {
            if let Some(m) = &env.payload.model {
                if let Some(info) = self.sessions.get_mut(&key) {
                    info.model = Some(m.clone());
                }
            }
        }

        let mut msgs = Vec::new();
        let run_id = self.route(env, &key, &session_id, ts, &mut msgs)?;

        // A single task change becomes the run's whole plan, so storage,
        // training and forecasts all see plans the same way.
        let with_plan;
        let env = match (&env.payload.task_id, &run_id) {
            (Some(task), Some(run)) if env.payload.plan.is_none() => {
                let mut e = env.clone();
                e.payload.plan = self
                    .trackers
                    .get_mut(run)
                    .and_then(|t| t.apply_task(task, env.payload.status.as_deref()));
                with_plan = e;
                &with_plan
            }
            _ => env,
        };

        let db = Instant::now();
        let mut meta = serde_json::to_value(&env.payload).unwrap_or_default();
        if let Some(o) = meta.as_object_mut() {
            // Stored elsewhere, or never stored: the run row keeps prompt
            // features, titles and effort; paths stay in memory.
            for k in [
                "title",
                "toolName",
                "activity",
                "durationMs",
                "status",
                "prompt",
                "effort",
                "permissionMode",
                "transcriptPath",
                "cwd",
                "agentProcess",
            ] {
                o.remove(k);
            }
        }
        let inserted = self.store.insert_event(&EventRow {
            id: env.event_id.clone(),
            run_id: run_id.clone(),
            session_id: Some(session_id.clone()),
            event_type: env.event_type,
            source_timestamp: ts,
            received_timestamp: received,
            activity: env.payload.activity,
            tool: env.payload.tool_name.clone(),
            duration_ms: env.payload.duration_ms,
            status: env.payload.status.clone(),
            metadata: meta,
        })?;
        self.stats
            .db_latency
            .push(db.elapsed().as_secs_f64() * 1000.0);
        if !inserted {
            self.stats.duplicates += 1;
            return Ok(IngestOutcome {
                duplicate: true,
                messages: msgs,
            });
        }

        if let Some(run_id) = run_id {
            self.apply(env, &run_id, ts, received, &mut msgs)?;
        }
        Ok(IngestOutcome {
            duplicate: false,
            messages: msgs,
        })
    }

    fn ensure_session(
        &mut self,
        key: &(AgentSource, String),
        ts: DateTime<Utc>,
        repo: Option<&str>,
    ) -> Result<String> {
        if let Some(info) = self.sessions.get(key) {
            let id = info.id.clone();
            self.store.touch_session(&id, ts, repo, "active")?;
            return Ok(id);
        }
        let id = match self.store.find_session(key.0, &key.1)? {
            Some(s) => {
                self.store.touch_session(&s.id, ts, repo, "active")?;
                s.id
            }
            None => {
                let s = SessionRow {
                    id: format!("ses_{}", uuid::Uuid::new_v4().simple()),
                    agent: key.0,
                    source_session_id: key.1.clone(),
                    repository_id: repo.map(String::from),
                    started_at: ts,
                    last_seen_at: ts,
                    status: "active".into(),
                };
                self.store.insert_session(&s)?;
                s.id
            }
        };
        self.sessions.insert(
            key.clone(),
            SessionInfo {
                id: id.clone(),
                ..Default::default()
            },
        );
        Ok(id)
    }

    fn open_run_of(&self, key: &(AgentSource, String)) -> Option<String> {
        self.sessions
            .get(key)
            .and_then(|s| s.open_run.clone())
            .filter(|id| self.trackers.contains_key(id))
    }

    fn route(
        &mut self,
        env: &EventEnvelope,
        key: &(AgentSource, String),
        session_id: &str,
        ts: DateTime<Utc>,
        msgs: &mut Vec<StreamMessage>,
    ) -> Result<Option<String>> {
        use EventType::*;
        let open = self.open_run_of(key);
        match env.event_type {
            SessionStarted => Ok(None),
            SessionEnded => {
                if let Some(id) = open {
                    self.finish(&id, RunState::Cancelled, ExitReason::SessionEnded, ts, msgs)?;
                }
                if let Some(s) = self.sessions.get(key) {
                    self.store.touch_session(&s.id, ts, None, "ended")?;
                }
                Ok(None)
            }
            AgentIdle => {
                if let Some(id) = open {
                    let t = &self.trackers[&id];
                    // Idle at the prompt with no turn end reported: the turn was interrupted.
                    if t.state().is_working() && t.row.started_at <= ts {
                        let end = t.row.last_event_at;
                        self.finish(
                            &id,
                            RunState::Cancelled,
                            ExitReason::IdleWithoutCompletion,
                            end,
                            msgs,
                        )?;
                    }
                }
                Ok(None)
            }
            PromptSubmitted | RunStarted => {
                if let Some(turn) = &env.source_turn_id {
                    if let Some(id) = self.run_for_turn(session_id, turn)? {
                        return Ok(Some(id));
                    }
                }
                if let Some(id) = open {
                    let started = self.trackers[&id].row.started_at;
                    if started > ts {
                        // A late prompt event from before the current run: record only.
                        return Ok(None);
                    }
                    if env.source_turn_id.is_none() && self.continues(&id, env, ts) {
                        return Ok(Some(id));
                    }
                    let end = self.trackers[&id].row.last_event_at;
                    self.finish(&id, RunState::Cancelled, ExitReason::Superseded, end, msgs)?;
                }
                Ok(Some(self.create_run(env, key, session_id, ts)?))
            }
            _ => {
                if let Some(turn) = &env.source_turn_id {
                    if let Some(id) = self.run_for_turn(session_id, turn)? {
                        return Ok(Some(id));
                    }
                    if env.event_type.is_terminal() {
                        return Ok(None);
                    }
                    // A new turn we never saw start (missed prompt event).
                    if let Some(id) = open {
                        let end = self.trackers[&id].row.last_event_at;
                        self.finish(&id, RunState::Cancelled, ExitReason::Superseded, end, msgs)?;
                    }
                    return Ok(Some(self.create_run(env, key, session_id, ts)?));
                }
                if let Some(id) = &open {
                    if self.trackers[id].row.started_at <= ts + Duration::seconds(2) {
                        return Ok(Some(id.clone()));
                    }
                }
                // Late event, or activity outside any open run.
                let recent = self.store.recent_runs_for_session(session_id, 20)?;
                if let Some(r) = recent
                    .iter()
                    .find(|r| r.started_at <= ts && r.ended_at.is_none_or(|e| e >= ts))
                {
                    return Ok(Some(r.id.clone()));
                }
                let activity = matches!(
                    env.event_type,
                    ToolStarted
                        | ToolCompleted
                        | ToolFailed
                        | ApprovalRequested
                        | SubagentStarted
                        | PlanUpdated
                );
                if activity && open.is_none() {
                    // The agent is working but we missed the start of the turn.
                    return Ok(Some(self.create_run(env, key, session_id, ts)?));
                }
                Ok(open)
            }
        }
    }

    /// Whether a prompt joins the open run instead of ending it.
    fn continues(&self, id: &str, env: &EventEnvelope, ts: DateTime<Utc>) -> bool {
        let t = &self.trackers[id];
        if !(t.state().is_working() || t.state() == RunState::WaitingForUser) {
            return false;
        }
        env.payload.injected == Some(true)
            || ts - t.row.last_event_at <= CONTINUE_WITHIN
            || t.tool_starts
                .values()
                .any(|s| ts - *s <= TOOL_IN_FLIGHT_MAX)
    }

    fn run_for_turn(&self, session_id: &str, turn: &str) -> Result<Option<String>> {
        if let Some((id, _)) = self.trackers.iter().find(|(_, t)| {
            t.row.session_id == session_id && t.row.source_turn_id.as_deref() == Some(turn)
        }) {
            return Ok(Some(id.clone()));
        }
        Ok(self.store.find_run_by_turn(session_id, turn)?.map(|r| r.id))
    }

    fn create_run(
        &mut self,
        env: &EventEnvelope,
        key: &(AgentSource, String),
        session_id: &str,
        ts: DateTime<Utc>,
    ) -> Result<String> {
        let info = self.sessions.get(key).cloned().unwrap_or_default();
        let mut features = env.payload.prompt.clone();
        if !self.prefs.similar_prompts {
            if let Some(f) = features.as_mut() {
                f.sketch = None;
            }
        }
        let row = RunRow {
            id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            session_id: session_id.into(),
            source_turn_id: env.source_turn_id.clone(),
            agent: env.source,
            repository_id: env.payload.repository.as_ref().map(|r| r.id.clone()),
            task_category: features
                .as_ref()
                .map(|f| f.category)
                .unwrap_or(TaskCategory::Unknown),
            prompt_features: features,
            title: if self.prefs.store_task_titles {
                env.payload.title.clone()
            } else {
                None
            },
            model: env.payload.model.clone().or(info.model.clone()),
            started_at: ts,
            ended_at: None,
            last_event_at: ts,
            wall_duration_ms: None,
            active_duration_ms: None,
            waiting_duration_ms: Some(0),
            status: RunState::Created,
            exit_reason: None,
            current_activity: None,
            current_tool: None,
            tool_calls: 0,
            tool_failures: 0,
            test_runs: 0,
            plan_total: None,
            plan_completed: None,
            initial_p10_ms: None,
            initial_p50_ms: None,
            initial_p90_ms: None,
            effort: env.payload.effort.clone(),
            permission_mode: env.payload.permission_mode.clone(),
            signals: None,
        };
        self.store.upsert_run(&row)?;
        let id = row.id.clone();
        let mut t = Tracker::new(row);
        t.launcher_id = env.payload.launcher_id.clone().or(info.launcher_id);
        t.agent_process = env.payload.agent_process.or(info.process);
        self.trackers.insert(id.clone(), t);
        if let Some(s) = self.sessions.get_mut(key) {
            s.open_run = Some(id.clone());
        }
        Ok(id)
    }

    fn set_state(t: &mut Tracker, to: RunState) {
        if t.row.status == to {
            return;
        }
        match t.row.status.transition(to) {
            Ok(s) => t.row.status = s,
            Err(e) => tracing::debug!("{e}"),
        }
    }

    /// Paths and settings the agent reports with every hook call.
    fn session_context(prefs: &Preferences, t: &mut Tracker, env: &EventEnvelope) {
        let p = &env.payload;
        if let Some(e) = &p.effort {
            t.row.effort = Some(e.clone());
        }
        if let Some(m) = &p.permission_mode {
            t.row.permission_mode = Some(m.clone());
        }
        if let Some(cwd) = &p.cwd {
            t.cwd = Some(PathBuf::from(cwd));
        }
        if !prefs.read_transcripts {
            return;
        }
        if let (Some(path), Some(format)) = (
            &p.transcript_path,
            TranscriptFormat::for_source(t.row.agent),
        ) {
            let path = PathBuf::from(path);
            // Agents write transcripts as JSON Lines; nothing else is opened.
            let jsonl = path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("jsonl"));
            if jsonl && t.transcript_path.as_ref() != Some(&path) {
                t.transcript = Some(TranscriptCursor::new(
                    path.clone(),
                    format,
                    t.row.started_at,
                ));
                t.transcript_path = Some(path);
            }
        }
    }

    fn apply(
        &mut self,
        env: &EventEnvelope,
        run_id: &str,
        ts: DateTime<Utc>,
        now: DateTime<Utc>,
        msgs: &mut Vec<StreamMessage>,
    ) -> Result<()> {
        use EventType::*;
        let Some(t) = self.trackers.get_mut(run_id) else {
            return Ok(());
        };
        if t.state().is_terminal() {
            return Ok(());
        }
        let ts = ts.max(t.row.started_at);
        t.row.last_event_at = t.row.last_event_at.max(ts);
        let p = &env.payload;
        Self::session_context(&self.prefs, t, env);

        // Any agent activity proves the run is alive again.
        if t.state() == RunState::Unknown && env.event_type != Heartbeat {
            Self::set_state(t, RunState::Running);
            t.row.exit_reason = None;
            if t.attention == Some(AttentionKind::ConnectionLost) {
                t.attention = None;
            }
        }
        if t.state() == RunState::Created {
            Self::set_state(t, RunState::Running);
        }
        let resumes = matches!(
            env.event_type,
            ToolStarted
                | ToolCompleted
                | ToolFailed
                | SubagentStarted
                | SubagentCompleted
                | PlanUpdated
                | ContextCompacting
                | ApprovalResolved
        );
        if resumes && t.state() == RunState::WaitingForUser {
            t.end_waiting(ts);
            Self::set_state(t, RunState::Running);
        }
        if let Some(a) = p
            .activity
            .filter(|a| *a != ActivityCategory::WaitingForApproval)
        {
            t.row.current_activity = Some(a);
        }
        if let Some(tool) = &p.tool_name {
            t.row.current_tool = Some(tool.clone());
        }
        if let Some(m) = &p.model {
            t.row.model = Some(m.clone());
        }
        if p.agent_process.is_some() {
            t.agent_process = p.agent_process;
        }
        if learning::on_timeline(env.event_type) {
            t.record(learning::event_from_envelope(env, ts));
        }

        let mut terminal: Option<(RunState, ExitReason)> = None;
        let mut attention = false;
        let mut command: Option<(String, f64, bool)> = None;
        let mut signals_changed = false;
        match env.event_type {
            PromptSubmitted | RunStarted => {
                // The first prompt describes the task; later ones continue it.
                if let Some(f) = p
                    .prompt
                    .as_ref()
                    .filter(|_| t.row.prompt_features.is_none())
                {
                    let mut f = f.clone();
                    if !self.prefs.similar_prompts {
                        f.sketch = None;
                    }
                    t.row.task_category = f.category;
                    t.row.prompt_features = Some(f);
                }
                if self.prefs.store_task_titles && t.row.title.is_none() && p.title.is_some() {
                    t.row.title = p.title.clone();
                }
            }
            ToolStarted => {
                if let Some(id) = &p.tool_use_id {
                    if t.tool_starts.len() >= MAX_TOOL_STARTS {
                        t.tool_starts.clear();
                    }
                    t.tool_starts.insert(id.clone(), ts);
                }
            }
            ToolCompleted | ToolFailed => {
                let failed = env.event_type == ToolFailed;
                t.failures
                    .record(p.tool_name.as_deref(), p.command_key.as_deref(), failed);
                let started = p
                    .tool_use_id
                    .as_ref()
                    .and_then(|id| t.tool_starts.remove(id));
                if let Some(key) = &p.command_key {
                    let secs = p.duration_ms.map(|d| d as f64 / 1000.0).or_else(|| {
                        started.map(|s| (ts - s).num_milliseconds().max(0) as f64 / 1000.0)
                    });
                    if let Some(secs) = secs {
                        command = Some((key.clone(), secs, failed));
                    }
                }
                if let Some(e) = p.edit.filter(|_| !failed) {
                    t.signals.edit_lines = t
                        .signals
                        .edit_lines
                        .saturating_add(e.lines_added.saturating_add(e.lines_removed));
                    signals_changed = true;
                }
                if failed && p.is_interrupt == Some(true) {
                    terminal = Some((RunState::Cancelled, ExitReason::UserInterrupt));
                }
            }
            PlanUpdated => {
                if let Some(plan) = &p.plan {
                    t.row.plan_total = Some(plan.total_steps);
                    t.row.plan_completed = Some(plan.completed_steps);
                }
            }
            ApprovalRequested => {
                if t.state() != RunState::WaitingForUser {
                    t.begin_waiting(ts);
                    Self::set_state(t, RunState::WaitingForUser);
                    t.attention = Some(AttentionKind::ApprovalRequired);
                    t.approvals += 1;
                    attention = true;
                }
            }
            ContextCompacting => t.row.current_activity = Some(ActivityCategory::CompactingContext),
            RunCompleted => terminal = Some((RunState::Completed, ExitReason::AgentFinished)),
            RunFailed => terminal = Some((RunState::Failed, ExitReason::AgentError)),
            RunCancelled => terminal = Some((RunState::Cancelled, ExitReason::AgentAborted)),
            SubagentStarted | SubagentCompleted | ApprovalResolved | SessionStarted
            | SessionEnded | AgentIdle | Heartbeat => {}
        }
        let repo = t.row.repository_id.clone();

        if let Some((key, secs, failed)) = command {
            self.observe_command(repo.as_deref(), &key, secs, failed, ts);
        }
        if signals_changed {
            self.snapshot_signals(run_id, now);
        }
        if let Some((state, reason)) = terminal {
            return self.finish(run_id, state, reason, ts, msgs);
        }
        if attention {
            self.notify(run_id, NotificationKind::AttentionRequired, msgs);
        }
        self.check_stuck(run_id, now, msgs);
        self.forecast(run_id, now, true);
        self.persist(run_id, now)?;
        msgs.push(self.run_message(run_id, now));
        Ok(())
    }

    fn observe_command(
        &mut self,
        repo: Option<&str>,
        key: &str,
        secs: f64,
        failed: bool,
        at: DateTime<Utc>,
    ) {
        let repo = repo.unwrap_or("").to_string();
        let stat = self
            .commands
            .entry((repo.clone(), key.to_string()))
            .or_insert_with(|| CommandStat::new(&repo, key));
        stat.observe(secs, failed);
        let row = CommandStatRow {
            repository_id: repo.clone(),
            command_key: key.to_string(),
            samples: stat.samples,
            log_mean: stat.log_mean,
            log_m2: stat.log_m2,
            failures: stat.failures,
            last_seen_at: at,
        };
        if let Err(e) = self.store.upsert_command_stat(&row) {
            self.stats
                .error(format!("could not store command timing: {e}"));
        }
        self.estimator.observe_command(
            Some(repo.as_str()).filter(|r| !r.is_empty()),
            key,
            secs,
            failed,
        );
    }

    /// Stores the run's signals for training, at most every
    /// [`SIGNAL_SNAPSHOT_EVERY_MS`] of active time.
    fn snapshot_signals(&mut self, run_id: &str, now: DateTime<Utc>) {
        let Some(t) = self.trackers.get_mut(run_id) else {
            return;
        };
        let active = t.active_ms(now);
        if t.last_signal_snapshot_ms
            .is_some_and(|l| active < l + SIGNAL_SNAPSHOT_EVERY_MS)
        {
            return;
        }
        t.last_signal_snapshot_ms = Some(active);
        if let Err(e) = self.store.insert_run_signals(run_id, active, &t.signals) {
            self.stats
                .error(format!("could not store run signals: {e}"));
        }
    }

    fn persist(&mut self, run_id: &str, now: DateTime<Utc>) -> Result<()> {
        if let Some(t) = self.trackers.get_mut(run_id) {
            t.sync_row(now);
            self.store.upsert_run(&t.row)?;
        }
        Ok(())
    }

    /// Moves a run to a terminal state. Idempotent for already-finished runs.
    fn finish(
        &mut self,
        run_id: &str,
        state: RunState,
        reason: ExitReason,
        ts: DateTime<Utc>,
        msgs: &mut Vec<StreamMessage>,
    ) -> Result<()> {
        let Some(mut t) = self.trackers.remove(run_id) else {
            return Ok(());
        };
        let end = ts.max(t.row.started_at);
        t.end_waiting(end);
        t.attention = None;
        if !t.row.status.can_transition_to(state) {
            self.trackers.insert(run_id.to_string(), t);
            return Ok(());
        }
        t.row.status = state;
        t.row.ended_at = Some(end);
        t.row.last_event_at = t.row.last_event_at.max(end);
        t.row.exit_reason = Some(reason.as_str().into());
        t.sync_row(end);
        self.store.upsert_run(&t.row)?;
        for s in self.sessions.values_mut() {
            if s.open_run.as_deref() == Some(run_id) {
                s.open_run = None;
            }
        }
        let name = self.repo_name(t.row.repository_id.as_deref());
        msgs.push(StreamMessage::RunUpdated {
            run: to_view(&t.row, end, name, None),
        });

        let wall = t.row.wall_duration_ms.unwrap_or(0);
        let long_enough = wall >= self.prefs.notify_min_duration_seconds as u64 * 1000;
        match state {
            RunState::Completed if long_enough => {
                self.notify_row(&t, NotificationKind::RunFinished, msgs)
            }
            RunState::Failed if long_enough => {
                self.notify_row(&t, NotificationKind::RunFailed, msgs)
            }
            _ => {}
        }
        self.training_changed();
        self.training.finished_since_fit += 1;
        self.finished_since_eval += 1;
        if self.finished_since_eval >= 10 {
            self.refresh_calibration();
        }
        Ok(())
    }

    // ----------------------------------------------------------- forecasts

    /// Updates the measured P10–P90 coverage used for confidence labels.
    pub fn refresh_calibration(&mut self) {
        self.finished_since_eval = 0;
        if let Ok(report) = self.accuracy() {
            self.measured_coverage = report.interval_coverage;
        }
    }

    fn forecast(&mut self, run_id: &str, now: DateTime<Utc>, force: bool) -> bool {
        self.ensure_trained();
        let started = Instant::now();
        let Some(t) = self.trackers.get(run_id) else {
            return false;
        };
        if !force
            && t.last_forecast
                .as_ref()
                .is_some_and(|f| now - f.generated_at < FORECAST_REFRESH)
        {
            return false;
        }
        let state = match t.state() {
            RunState::Created | RunState::Estimating | RunState::Blocked => RunState::Running,
            s => s,
        };
        let snapshot = t.snapshot(now);
        let signals = Some(&t.signals).filter(|s| **s != RunSignals::default());
        let input = ForecastInput {
            run_id,
            now,
            agent: t.row.agent,
            repository_id: t.row.repository_id.as_deref(),
            prompt: t.row.prompt_features.as_ref(),
            effort: t.row.effort.as_deref(),
            model: t.row.model.as_deref(),
            state,
            snapshot: &snapshot,
            signals,
            repo_files: t
                .row
                .repository_id
                .as_ref()
                .and_then(|r| self.repo_files.get(r).copied()),
            measured_coverage: self.measured_coverage,
        };
        let raw = self.estimator.forecast(&input);
        let f = smooth(t.last_forecast.as_ref(), raw);
        self.stats
            .forecast_latency
            .push(started.elapsed().as_secs_f64() * 1000.0);

        let t = self.trackers.get_mut(run_id).expect("checked above");
        let changed = t.last_forecast.as_ref().map(|p| {
            p.status != f.status
                || p.remaining_p50_seconds != f.remaining_p50_seconds
                || p.remaining_p90_seconds != f.remaining_p90_seconds
        });
        let persist = match &t.last_persisted {
            None => true,
            Some(p) => {
                let ratio = match (p.remaining_p50_seconds, f.remaining_p50_seconds) {
                    (Some(a), Some(b)) => (b.max(1) as f64 / a.max(1) as f64).ln().abs(),
                    _ => 0.0,
                };
                p.status != f.status
                    || f.reason_codes
                        .iter()
                        .any(|r| r == "estimate_increased" || r == "estimate_decreased")
                    || ratio > 0.15
                    || f.generated_at - p.generated_at >= FORECAST_PERSIST_EVERY
            }
        };
        if t.row.initial_p50_ms.is_none() && f.remaining_p50_seconds.is_some() {
            t.row.initial_p10_ms = f.remaining_p10_seconds.map(|s| s as u64 * 1000);
            t.row.initial_p50_ms = f.remaining_p50_seconds.map(|s| s as u64 * 1000);
            t.row.initial_p90_ms = f.remaining_p90_seconds.map(|s| s as u64 * 1000);
        }
        if persist {
            if let Err(e) = self.store.insert_forecast(&f) {
                self.stats.error(format!("could not store forecast: {e}"));
            }
            t.last_persisted = Some(f.clone());
        }
        t.last_forecast = Some(f);
        changed.unwrap_or(true)
    }

    // -------------------------------------------------------------- probes

    /// Transcript reads and `git` samples due for open runs. Each request
    /// takes the run's transcript cursor until [`Engine::apply_probe`] returns it.
    pub fn probe_requests(&mut self, now: DateTime<Utc>) -> Vec<ProbeRequest> {
        let mut out = Vec::new();
        let read_transcripts = self.prefs.read_transcripts;
        let sample_git = self.prefs.sample_git_diff;
        for (id, t) in self.trackers.iter_mut() {
            if t.probing || t.state().is_terminal() || t.state() == RunState::Unknown {
                continue;
            }
            let transcript = if read_transcripts {
                t.transcript.take()
            } else {
                None
            };
            let git = match &t.cwd {
                Some(cwd)
                    if sample_git
                        && t.last_git_probe.is_none_or(|g| now - g >= GIT_PROBE_EVERY) =>
                {
                    let count_files = t.row.repository_id.as_ref().is_some_and(|r| {
                        self.files_counted
                            .get(r)
                            .is_none_or(|at| now - *at >= FILE_COUNT_EVERY)
                    });
                    if count_files {
                        if let Some(r) = &t.row.repository_id {
                            self.files_counted.insert(r.clone(), now);
                        }
                    }
                    t.last_git_probe = Some(now);
                    Some(GitProbe {
                        cwd: cwd.clone(),
                        baseline: t.git_baseline.take(),
                        count_files,
                    })
                }
                _ => None,
            };
            if transcript.is_none() && git.is_none() {
                continue;
            }
            t.probing = true;
            out.push(ProbeRequest {
                run_id: id.clone(),
                repository_id: t.row.repository_id.clone(),
                transcript,
                git,
            });
        }
        out
    }

    pub fn apply_probe(&mut self, r: ProbeResult, now: DateTime<Utc>) -> Vec<StreamMessage> {
        if let (Some(n), Some(repo)) = (r.file_count, &r.repository_id) {
            self.repo_files.insert(repo.clone(), n);
            if let Err(e) = self.store.set_repository_file_count(repo, n) {
                self.stats.error(format!("could not store file count: {e}"));
            }
        }
        let read_transcripts = self.prefs.read_transcripts;
        let sample_git = self.prefs.sample_git_diff;
        let Some(t) = self.trackers.get_mut(&r.run_id) else {
            return Vec::new();
        };
        t.probing = false;
        let mut changed = false;
        if let Some(cursor) = r.transcript {
            if read_transcripts
                && t.transcript_path.as_ref() == Some(&cursor.path)
                && t.transcript.is_none()
            {
                if r.transcript_changed {
                    cursor.fill(&mut t.signals);
                    if t.row.effort.is_none() {
                        t.row.effort = cursor.effort.clone();
                    }
                    if t.row.model.is_none() {
                        t.row.model = cursor.model.clone();
                    }
                    // The session's next run starts with the model known.
                    if let Some(m) = &t.row.model {
                        let session = &t.row.session_id;
                        if let Some(info) = self.sessions.values_mut().find(|s| &s.id == session) {
                            info.model = Some(m.clone());
                        }
                    }
                    changed = true;
                }
                t.transcript = Some(cursor);
            }
        }
        if sample_git {
            if r.baseline.is_some() {
                t.git_baseline = r.baseline;
            }
            if let Some((files, lines)) = r.diff {
                if t.signals.diff_files != Some(files) || t.signals.diff_lines != Some(lines) {
                    t.signals.diff_files = Some(files);
                    t.signals.diff_lines = Some(lines);
                    changed = true;
                }
            }
        }
        if !changed {
            return Vec::new();
        }
        let id = r.run_id;
        self.snapshot_signals(&id, now);
        self.forecast(&id, now, false);
        if let Err(e) = self.persist(&id, now) {
            self.stats.error(e.to_string());
        }
        vec![self.run_message(&id, now)]
    }

    /// Runs due probes on this thread. The server does the same off the lock.
    pub fn run_probes(&mut self, now: DateTime<Utc>) -> Vec<StreamMessage> {
        let requests = self.probe_requests(now);
        requests
            .into_iter()
            .flat_map(|req| {
                let res = probe::run(req);
                self.apply_probe(res, now)
            })
            .collect()
    }

    // --------------------------------------------------------------- ticks

    /// Periodic maintenance: staleness, eviction, and forecast refresh.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<StreamMessage> {
        let mut msgs = Vec::new();
        let ids: Vec<String> = self.trackers.keys().cloned().collect();
        for id in ids {
            let (state, last) = {
                let t = &self.trackers[&id];
                (t.state(), t.row.last_event_at)
            };
            if state == RunState::Unknown && now - last > EVICT_UNKNOWN_AFTER {
                let _ = self.persist(&id, now);
                self.trackers.remove(&id);
                continue;
            }
            if self.agent_exited(&id, now) {
                if let Err(e) = self.finish(
                    &id,
                    RunState::Cancelled,
                    ExitReason::ProcessExited,
                    now,
                    &mut msgs,
                ) {
                    self.stats.error(e.to_string());
                }
                continue;
            }
            if state.is_working() && now - last > STALE_AFTER {
                if let Some(t) = self.trackers.get_mut(&id) {
                    Self::set_state(t, RunState::Unknown);
                    t.row.exit_reason = Some(ExitReason::Stale.as_str().into());
                    t.attention = Some(AttentionKind::ConnectionLost);
                }
                self.notify(&id, NotificationKind::ConnectionLost, &mut msgs);
                self.forecast(&id, now, true);
                if let Err(e) = self.persist(&id, now) {
                    self.stats.error(e.to_string());
                }
                msgs.push(self.run_message(&id, now));
                continue;
            }
            let stuck_changed = self.check_stuck(&id, now, &mut msgs);
            let refreshed = state.is_working() && self.forecast(&id, now, false);
            if stuck_changed || refreshed {
                if let Err(e) = self.persist(&id, now) {
                    self.stats.error(e.to_string());
                }
                msgs.push(self.run_message(&id, now));
            }
        }
        msgs
    }

    /// The run's agent process has exited, and the run went quiet before it.
    fn agent_exited(&self, id: &str, now: DateTime<Utc>) -> bool {
        let t = &self.trackers[id];
        t.state().is_open()
            && now - t.row.last_event_at > PROCESS_QUIET
            && t.agent_process.is_some_and(|p| !(self.process_running)(&p))
    }

    /// The `runmarq run` wrapper reports that the agent process exited.
    pub fn launcher_exited(
        &mut self,
        launcher_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<StreamMessage>> {
        let mut msgs = Vec::new();
        let ids: Vec<String> = self
            .trackers
            .iter()
            .filter(|(_, t)| t.launcher_id.as_deref() == Some(launcher_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.finish(
                &id,
                RunState::Cancelled,
                ExitReason::ProcessExited,
                now,
                &mut msgs,
            )?;
        }
        Ok(msgs)
    }

    /// Starts, updates or ends a working run's stuck episode, notifying once
    /// per episode. Returns whether the run's view changed.
    fn check_stuck(&mut self, id: &str, now: DateTime<Utc>, msgs: &mut Vec<StreamMessage>) -> bool {
        let Some(t) = self.trackers.get(id) else {
            return false;
        };
        if !t.state().is_working() {
            return false;
        }
        let found = stuck::detect(&stuck::evidence(t, &self.commands, now));
        let t = self.trackers.get_mut(id).expect("checked above");
        let before = (t.attention, t.stuck.clone());
        let started = found.is_some() && t.stuck.is_none();
        if started {
            t.stuck_episodes += 1;
        }
        t.stuck = found;
        match (&t.stuck, t.attention) {
            (Some(_), None) => t.attention = Some(AttentionKind::Stuck),
            (None, Some(AttentionKind::Stuck)) => t.attention = None,
            _ => {}
        }
        let changed = before != (t.attention, t.stuck.clone());
        if started {
            self.notify(id, NotificationKind::Stuck, msgs);
        }
        changed
    }

    // ------------------------------------------------------- notifications

    fn notify(&mut self, run_id: &str, kind: NotificationKind, msgs: &mut Vec<StreamMessage>) {
        if let Some(t) = self.trackers.get(run_id).cloned() {
            self.notify_row(&t, kind, msgs);
        }
    }

    fn notify_row(&mut self, t: &Tracker, kind: NotificationKind, msgs: &mut Vec<StreamMessage>) {
        let p = &self.prefs;
        let wanted = p.notifications_enabled
            && !p.quiet_mode
            && match kind {
                NotificationKind::RunFinished => p.notify_on_finish,
                NotificationKind::RunFailed => p.notify_on_failure,
                NotificationKind::AttentionRequired => p.notify_on_attention,
                NotificationKind::ConnectionLost => p.notify_on_disconnect,
                NotificationKind::Stuck => p.notify_on_stuck,
            };
        if !wanted {
            return;
        }
        let key = match kind {
            NotificationKind::AttentionRequired => format!("{}:{}", kind.as_str(), t.approvals),
            NotificationKind::Stuck => format!("{}:{}", kind.as_str(), t.stuck_episodes),
            _ => kind.as_str().to_string(),
        };
        match self.store.record_notification(&t.row.id, &key, Utc::now()) {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                self.stats.error(format!("notification log: {e}"));
                return;
            }
        }
        let agent = t.row.agent.display_name();
        let place = self
            .repo_name(t.row.repository_id.as_deref())
            .map(|n| format!(" in {n}"))
            .unwrap_or_default();
        let what = t.row.title.clone().unwrap_or_else(|| "Run".into());
        let dur = human_duration(
            t.row
                .wall_duration_ms
                .unwrap_or_else(|| t.wall_ms(Utc::now())),
        );
        let (title, body) = match kind {
            NotificationKind::RunFinished => (
                format!("{agent} finished"),
                format!("{what}{place} · {dur}"),
            ),
            NotificationKind::RunFailed => (
                format!("{agent} run failed"),
                format!("{what}{place} · stopped after {dur}"),
            ),
            NotificationKind::AttentionRequired => (
                format!("{agent} is waiting for you"),
                format!("Approval needed{place}"),
            ),
            NotificationKind::ConnectionLost => (
                format!("No updates from {agent}"),
                format!(
                    "Status unknown for a run{place} after {} min of silence",
                    STALE_AFTER.num_minutes()
                ),
            ),
            NotificationKind::Stuck => (
                format!("{agent} may be stuck{place}"),
                t.stuck
                    .as_ref()
                    .map(|s| s.detail.clone())
                    .unwrap_or_else(|| "No progress for a while".into()),
            ),
        };
        msgs.push(StreamMessage::Notification {
            notification: NotificationRequest {
                id: format!("{}:{key}", t.row.id),
                run_id: t.row.id.clone(),
                kind,
                title,
                body,
            },
        });
    }

    // ------------------------------------------------------------- queries

    fn repo_name(&mut self, id: Option<&str>) -> Option<String> {
        let id = id?;
        if !self.prefs.store_repository_names {
            return None;
        }
        if let Some(n) = self.repo_names.get(id) {
            return n.clone();
        }
        let n = self.store.repository_name(id).ok().flatten();
        self.repo_names.insert(id.to_string(), n.clone());
        n
    }

    fn run_message(&mut self, run_id: &str, now: DateTime<Utc>) -> StreamMessage {
        let repo = self
            .trackers
            .get(run_id)
            .and_then(|t| t.row.repository_id.clone());
        let name = self.repo_name(repo.as_deref());
        let t = &self.trackers[run_id];
        StreamMessage::RunUpdated {
            run: t.view(now, name),
        }
    }

    pub fn active_runs(&mut self, now: DateTime<Utc>) -> Vec<RunView> {
        let mut ids: Vec<(DateTime<Utc>, String)> = self
            .trackers
            .iter()
            .map(|(id, t)| (t.row.started_at, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter()
            .map(|(_, id)| match self.run_message(&id, now) {
                StreamMessage::RunUpdated { run } => run,
                _ => unreachable!(),
            })
            .collect()
    }

    pub fn snapshot(&mut self, now: DateTime<Utc>) -> StreamMessage {
        StreamMessage::Snapshot {
            runs: self.active_runs(now),
        }
    }

    /// Open runs as they stand, with signals read since they were last stored.
    pub fn open_rows(&self) -> Vec<RunRow> {
        self.trackers
            .values()
            .map(|t| {
                let mut row = t.row.clone();
                if t.signals != RunSignals::default() {
                    row.signals = Some(t.signals.clone());
                }
                row
            })
            .collect()
    }

    pub fn view_of(&mut self, row: &RunRow, now: DateTime<Utc>) -> RunView {
        let name = self.repo_name(row.repository_id.as_deref());
        match self.trackers.get(&row.id) {
            Some(t) => t.view(now, name),
            None => to_view(row, now, name, None),
        }
    }

    pub fn history(&mut self, q: &HistoryQuery, now: DateTime<Utc>) -> Result<HistoryPage> {
        let (rows, total) = self.store.list_runs(q)?;
        let runs = rows.iter().map(|r| self.view_of(r, now)).collect();
        Ok(HistoryPage { runs, total })
    }

    pub fn run_detail(&mut self, id: &str, now: DateTime<Utc>) -> Result<Option<RunDetail>> {
        let row = match self.trackers.get(id) {
            Some(t) => t.row.clone(),
            None => match self.store.get_run(id)? {
                Some(r) => r,
                None => return Ok(None),
            },
        };
        let run = self.view_of(&row, now);
        let events = self.store.events_for_run(id)?;
        let forecasts = self.store.forecasts_for_run(id)?;
        let changes = forecasts
            .iter()
            .filter_map(|f| {
                change_message(f).map(|message| ForecastChange {
                    at: f.generated_at,
                    message,
                })
            })
            .collect();
        let phases = phases(&events, row.ended_at);
        Ok(Some(RunDetail {
            run,
            events: events
                .iter()
                .map(|e| EventView {
                    id: e.id.clone(),
                    event_type: e.event_type,
                    source_timestamp: e.source_timestamp,
                    received_timestamp: e.received_timestamp,
                    activity: e.activity,
                    tool: e.tool.clone(),
                    duration_ms: e.duration_ms,
                    status: e.status.clone(),
                })
                .collect(),
            forecasts,
            phases,
            changes,
        }))
    }

    pub fn accuracy(&self) -> Result<runmarq_core::api::AccuracyReport> {
        let rows = self.store.finished_runs(500)?;
        let forecasts = self.store.forecasts_for_finished_runs(500)?;
        let mut by_run: HashMap<String, Vec<_>> = HashMap::new();
        for f in forecasts {
            by_run.entry(f.run_id.clone()).or_default().push(f);
        }
        let eval_runs: Vec<EvalRun> = rows
            .iter()
            .filter_map(|r| {
                Some(EvalRun {
                    run_id: r.id.clone(),
                    agent: r.agent,
                    category: r.task_category,
                    outcome: learning::outcome(r)?,
                    active_seconds: r.active_duration_ms? as f64 / 1000.0,
                    forecasts: by_run.remove(&r.id).unwrap_or_default(),
                })
            })
            .collect();
        Ok(evaluate(&eval_runs, self.estimator.holdout()))
    }

    pub fn insights(&mut self) -> Result<Insights> {
        self.ensure_trained();
        let rows = self.store.finished_runs(HISTORY_LIMIT)?;
        let completed: Vec<&RunRow> = rows
            .iter()
            .filter(|r| r.status == RunState::Completed)
            .collect();
        let mut durations: Vec<f64> = completed
            .iter()
            .filter_map(|r| r.wall_duration_ms)
            .map(|v| v as f64)
            .collect();
        let median = runmarq_estimator::stats::median(&mut durations).map(|v| v as u64);
        let total_waiting = rows.iter().filter_map(|r| r.waiting_duration_ms).sum();

        fn bucket(key: String, label: String, mut d: Vec<f64>) -> DurationBucket {
            let n = d.len() as u32;
            let enough = n >= 5;
            DurationBucket {
                key,
                label,
                runs: n,
                median_ms: if enough {
                    runmarq_estimator::stats::median(&mut d).map(|v| v as u64)
                } else {
                    None
                },
                p90_ms: if enough {
                    runmarq_estimator::stats::quantile(&mut d, 0.9).map(|v| v as u64)
                } else {
                    None
                },
            }
        }
        let mut by_agent: HashMap<AgentSource, Vec<f64>> = HashMap::new();
        let mut by_cat: HashMap<TaskCategory, Vec<f64>> = HashMap::new();
        for r in &completed {
            if let Some(a) = r.active_duration_ms {
                by_agent.entry(r.agent).or_default().push(a as f64);
                by_cat.entry(r.task_category).or_default().push(a as f64);
            }
        }
        let mut agents: Vec<DurationBucket> = by_agent
            .into_iter()
            .map(|(a, d)| bucket(a.as_str().into(), a.display_name().into(), d))
            .collect();
        agents.sort_by_key(|b| std::cmp::Reverse(b.runs));
        let mut cats: Vec<DurationBucket> = by_cat
            .into_iter()
            .map(|(c, d)| bucket(c.as_str().into(), c.label().into(), d))
            .collect();
        cats.sort_by_key(|b| std::cmp::Reverse(b.runs));

        Ok(Insights {
            total_runs: self.store.count_runs()?,
            finished_runs: rows.len() as u32,
            median_duration_ms: if completed.len() >= 3 { median } else { None },
            total_waiting_ms: total_waiting,
            by_agent: agents,
            by_category: cats,
            accuracy: self.accuracy()?,
            model: self.estimator.status(),
        })
    }

    pub fn set_preferences(&mut self, p: Preferences) -> Result<StreamMessage> {
        let was = self.prefs.clone();
        self.store.save_preferences(&p)?;
        if was.store_repository_names && !p.store_repository_names {
            self.store.clear_repository_names()?;
            self.repo_names.clear();
        }
        if was.store_task_titles && !p.store_task_titles {
            self.store.clear_titles()?;
            for t in self.trackers.values_mut() {
                t.row.title = None;
            }
        }
        if was.similar_prompts && !p.similar_prompts {
            self.store.clear_prompt_sketches()?;
            for t in self.trackers.values_mut() {
                if let Some(f) = t.row.prompt_features.as_mut() {
                    f.sketch = None;
                }
            }
            self.training_changed();
            self.training.urgent = true;
        }
        if !p.read_transcripts {
            for t in self.trackers.values_mut() {
                t.transcript = None;
                t.transcript_path = None;
            }
        }
        if !p.sample_git_diff {
            for t in self.trackers.values_mut() {
                t.git_baseline = None;
                t.cwd = None;
            }
        }
        self.prefs = p.clone();
        self.write_hook_config();
        Ok(StreamMessage::PreferencesChanged { preferences: p })
    }

    pub fn delete_history(&mut self, before: Option<DateTime<Utc>>) -> Result<usize> {
        let n = self.store_mut().delete_history(before)?;
        self.training_changed();
        self.training.urgent = true;
        self.refresh_calibration();
        Ok(n)
    }

    /// Applies the retention preference, if set.
    pub fn apply_retention(&mut self, now: DateTime<Utc>) -> Result<usize> {
        match self.prefs.retention_days {
            Some(days) if days > 0 => self.delete_history(Some(now - Duration::days(days as i64))),
            _ => Ok(0),
        }
    }
}

/// Groups consecutive events by activity into phases. Tool calls are
/// counted from start events, or from completions for adapters that only
/// report completions.
pub fn phases(events: &[EventRow], ended: Option<DateTime<Utc>>) -> Vec<PhaseSpan> {
    let has_starts = events
        .iter()
        .any(|e| e.event_type == EventType::ToolStarted);
    let mut out: Vec<PhaseSpan> = Vec::new();
    for e in events {
        let Some(a) = e.activity else { continue };
        let is_call = if has_starts {
            e.event_type == EventType::ToolStarted
        } else {
            matches!(
                e.event_type,
                EventType::ToolCompleted | EventType::ToolFailed
            )
        };
        match out.last_mut() {
            Some(last) if last.activity == a => last.tool_calls += u32::from(is_call),
            _ => {
                if let Some(last) = out.last_mut() {
                    last.ended_at = Some(e.source_timestamp);
                }
                out.push(PhaseSpan {
                    activity: a,
                    started_at: e.source_timestamp,
                    ended_at: None,
                    tool_calls: u32::from(is_call),
                });
            }
        }
    }
    if let Some(last) = out.last_mut() {
        last.ended_at = ended;
    }
    out
}
