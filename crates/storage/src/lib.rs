//! SQLite persistence. One `Store` owns one connection; every query is parameterized.

mod migrations;

use chrono::{DateTime, TimeZone, Utc};
use runmarq_core::api::{HistoryQuery, Preferences, RepositoryView};
use runmarq_core::event::{
    ActivityCategory, AgentSource, ComplexityBucket, EventType, PromptFeatures, TaskCategory,
};
use runmarq_core::forecast::{Confidence, EtaForecast, ForecastBasis, ForecastStatus};
use runmarq_core::run::RunState;
use runmarq_core::signals::RunSignals;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashMap;
use std::path::Path;

pub use migrations::MIGRATIONS;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("migration {version} failed: {source}")]
    Migration {
        version: u32,
        source: rusqlite::Error,
    },
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("invalid stored value in {column}: {value}")]
    Corrupt { column: &'static str, value: String },
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub fn to_ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

pub fn from_ms(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| Utc.timestamp_millis_opt(0).unwrap())
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub id: String,
    pub session_id: String,
    pub source_turn_id: Option<String>,
    pub agent: AgentSource,
    pub repository_id: Option<String>,
    pub task_category: TaskCategory,
    pub prompt_features: Option<PromptFeatures>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_event_at: DateTime<Utc>,
    pub wall_duration_ms: Option<u64>,
    pub active_duration_ms: Option<u64>,
    pub waiting_duration_ms: Option<u64>,
    pub status: RunState,
    pub exit_reason: Option<String>,
    pub current_activity: Option<ActivityCategory>,
    pub current_tool: Option<String>,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub test_runs: u32,
    pub plan_total: Option<u32>,
    pub plan_completed: Option<u32>,
    pub initial_p10_ms: Option<u64>,
    pub initial_p50_ms: Option<u64>,
    pub initial_p90_ms: Option<u64>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub signals: Option<RunSignals>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandStatRow {
    pub repository_id: String,
    pub command_key: String,
    pub samples: u32,
    /// Mean and sum of squared deviations of ln(duration in seconds).
    pub log_mean: f64,
    pub log_m2: f64,
    pub failures: u32,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalSnapshot {
    pub active_ms: u64,
    pub signals: RunSignals,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub id: String,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub event_type: EventType,
    pub source_timestamp: DateTime<Utc>,
    pub received_timestamp: DateTime<Utc>,
    pub activity: Option<ActivityCategory>,
    pub tool: Option<String>,
    pub duration_ms: Option<u64>,
    pub status: Option<String>,
    /// The minimized payload, minus fields stored in their own columns.
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub agent: AgentSource,
    pub source_session_id: String,
    pub repository_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallationRow {
    pub agent: AgentSource,
    pub scope: String,
    pub config_location: String,
    pub installed_version: Option<String>,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

pub struct Store {
    conn: Connection,
}

fn parse_enum<T>(column: &'static str, v: String, f: impl Fn(&str) -> Option<T>) -> Result<T> {
    f(&v).ok_or(StoreError::Corrupt { column, value: v })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL keeps readers (CLI, diagnostics) from blocking the collector's writes.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        let latest = MIGRATIONS.last().map(|m| m.0).unwrap_or(0);
        if current > latest {
            return Err(StoreError::SchemaTooNew {
                found: current,
                supported: latest,
            });
        }
        for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)
                .map_err(|source| StoreError::Migration {
                    version: *version,
                    source,
                })?;
            tx.pragma_update(None, "user_version", version)
                .map_err(|source| StoreError::Migration {
                    version: *version,
                    source,
                })?;
            tx.commit()?;
            tracing::info!(version, "applied database migration");
        }
        Ok(())
    }

    pub fn transaction<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ---------------------------------------------------------------- agents

    pub fn upsert_agent(
        &self,
        agent: AgentSource,
        installed_version: Option<&str>,
        integration_status: &str,
        capabilities_json: &str,
    ) -> Result<()> {
        let now = to_ms(Utc::now());
        self.conn.execute(
            "INSERT INTO agents (id, type, display_name, installed_version, integration_status, capabilities_json, created_at, updated_at)
             VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET installed_version = excluded.installed_version,
               integration_status = excluded.integration_status, capabilities_json = excluded.capabilities_json,
               updated_at = excluded.updated_at",
            params![agent.as_str(), agent.display_name(), installed_version, integration_status, capabilities_json, now],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------- repositories

    pub fn upsert_repository(
        &self,
        id: &str,
        display_name: Option<&str>,
        at: DateTime<Utc>,
    ) -> Result<()> {
        let at = to_ms(at);
        self.conn.execute(
            "INSERT INTO repositories (id, local_identifier, display_name, first_seen_at, last_seen_at)
             VALUES (?1, ?1, ?2, ?3, ?3)
             ON CONFLICT(id) DO UPDATE SET last_seen_at = MAX(last_seen_at, excluded.last_seen_at),
               display_name = COALESCE(excluded.display_name, display_name)",
            params![id, display_name, at],
        )?;
        Ok(())
    }

    pub fn repository_name(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT display_name FROM repositories WHERE id = ?1",
                [id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn list_repositories(&self) -> Result<Vec<RepositoryView>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.display_name, COUNT(runs.id) FROM repositories r
             LEFT JOIN runs ON runs.repository_id = r.id
             GROUP BY r.id ORDER BY MAX(r.last_seen_at) DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(RepositoryView {
                id: r.get(0)?,
                display_name: r.get(1)?,
                run_count: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Remove stored folder names (used when the user disables repository names).
    pub fn clear_repository_names(&self) -> Result<usize> {
        Ok(self
            .conn
            .execute("UPDATE repositories SET display_name = NULL", [])?)
    }

    pub fn clear_titles(&self) -> Result<usize> {
        Ok(self.conn.execute("UPDATE runs SET title = NULL", [])?)
    }

    // -------------------------------------------------------------- sessions

    pub fn find_session(
        &self,
        agent: AgentSource,
        source_session_id: &str,
    ) -> Result<Option<SessionRow>> {
        self.conn
            .query_row(
                "SELECT id, agent_id, source_session_id, repository_id, started_at, last_seen_at, status
                 FROM sessions WHERE agent_id = ?1 AND source_session_id = ?2",
                params![agent.as_str(), source_session_id],
                session_from_row,
            )
            .optional()?
            .transpose()
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionRow>> {
        self.conn
            .query_row(
                "SELECT id, agent_id, source_session_id, repository_id, started_at, last_seen_at, status
                 FROM sessions WHERE id = ?1",
                [id],
                session_from_row,
            )
            .optional()?
            .transpose()
    }

    pub fn insert_session(&self, s: &SessionRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, agent_id, source_session_id, repository_id, started_at, last_seen_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                s.id,
                s.agent.as_str(),
                s.source_session_id,
                s.repository_id,
                to_ms(s.started_at),
                to_ms(s.last_seen_at),
                s.status
            ],
        )?;
        Ok(())
    }

    pub fn touch_session(
        &self,
        id: &str,
        at: DateTime<Utc>,
        repository_id: Option<&str>,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET last_seen_at = MAX(last_seen_at, ?2), repository_id = COALESCE(?3, repository_id), status = ?4
             WHERE id = ?1",
            params![id, to_ms(at), repository_id, status],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ runs

    pub fn upsert_run(&self, r: &RunRow) -> Result<()> {
        let now = to_ms(Utc::now());
        let features = r
            .prompt_features
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let fingerprint = r
            .prompt_features
            .as_ref()
            .and_then(|f| f.fingerprint.clone());
        let complexity = r.prompt_features.as_ref().map(|f| f.complexity.as_str());
        let signals = r.signals.as_ref().map(serde_json::to_string).transpose()?;
        self.conn.execute(
            "INSERT INTO runs (id, session_id, source_turn_id, agent_id, repository_id, task_category, complexity_bucket,
                prompt_features_json, prompt_fingerprint, title, model, started_at, ended_at, last_event_at,
                wall_duration_ms, active_duration_ms, waiting_duration_ms, status, exit_reason, current_activity,
                current_tool, tool_calls, tool_failures, test_runs, plan_total, plan_completed,
                initial_p10_ms, initial_p50_ms, initial_p90_ms, effort, permission_mode, signals_json,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?33)
             ON CONFLICT(id) DO UPDATE SET
                source_turn_id = excluded.source_turn_id, repository_id = excluded.repository_id,
                task_category = excluded.task_category, complexity_bucket = excluded.complexity_bucket,
                prompt_features_json = excluded.prompt_features_json, prompt_fingerprint = excluded.prompt_fingerprint,
                title = excluded.title, model = excluded.model, ended_at = excluded.ended_at,
                last_event_at = excluded.last_event_at, wall_duration_ms = excluded.wall_duration_ms,
                active_duration_ms = excluded.active_duration_ms, waiting_duration_ms = excluded.waiting_duration_ms,
                status = excluded.status, exit_reason = excluded.exit_reason,
                current_activity = excluded.current_activity, current_tool = excluded.current_tool,
                tool_calls = excluded.tool_calls, tool_failures = excluded.tool_failures, test_runs = excluded.test_runs,
                plan_total = excluded.plan_total, plan_completed = excluded.plan_completed,
                initial_p10_ms = excluded.initial_p10_ms, initial_p50_ms = excluded.initial_p50_ms,
                initial_p90_ms = excluded.initial_p90_ms, effort = excluded.effort,
                permission_mode = excluded.permission_mode, signals_json = excluded.signals_json,
                updated_at = excluded.updated_at",
            params![
                r.id,
                r.session_id,
                r.source_turn_id,
                r.agent.as_str(),
                r.repository_id,
                r.task_category.as_str(),
                complexity,
                features,
                fingerprint,
                r.title,
                r.model,
                to_ms(r.started_at),
                r.ended_at.map(to_ms),
                to_ms(r.last_event_at),
                r.wall_duration_ms.map(|v| v as i64),
                r.active_duration_ms.map(|v| v as i64),
                r.waiting_duration_ms.map(|v| v as i64),
                r.status.as_str(),
                r.exit_reason,
                r.current_activity.map(|a| a.as_str()),
                r.current_tool,
                r.tool_calls,
                r.tool_failures,
                r.test_runs,
                r.plan_total,
                r.plan_completed,
                r.initial_p10_ms.map(|v| v as i64),
                r.initial_p50_ms.map(|v| v as i64),
                r.initial_p90_ms.map(|v| v as i64),
                r.effort,
                r.permission_mode,
                signals,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_run(&self, id: &str) -> Result<Option<RunRow>> {
        self.conn
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
                [id],
                run_from_row,
            )
            .optional()?
            .transpose()
    }

    pub fn find_run_by_turn(&self, session_id: &str, turn_id: &str) -> Result<Option<RunRow>> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {RUN_COLUMNS} FROM runs WHERE session_id = ?1 AND source_turn_id = ?2"
                ),
                params![session_id, turn_id],
                run_from_row,
            )
            .optional()?
            .transpose()
    }

    /// Most recent runs of a session, newest first.
    pub fn recent_runs_for_session(&self, session_id: &str, limit: u32) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE session_id = ?1 ORDER BY started_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![session_id, limit], run_from_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn begin(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn rollback(&self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }

    pub fn open_runs(&self) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE status NOT IN ('completed','failed','cancelled') ORDER BY started_at"
        ))?;
        let rows = stmt.query_map([], run_from_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn list_runs(&self, q: &HistoryQuery) -> Result<(Vec<RunRow>, u32)> {
        let mut clauses: Vec<&str> = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(a) = q.agent {
            clauses.push("runs.agent_id = ?");
            values.push(a.as_str().to_string().into());
        }
        if let Some(r) = &q.repository_id {
            clauses.push("runs.repository_id = ?");
            values.push(r.clone().into());
        }
        if let Some(s) = q.state {
            clauses.push("runs.status = ?");
            values.push(s.as_str().to_string().into());
        }
        if let Some(f) = q.from {
            clauses.push("runs.started_at >= ?");
            values.push(to_ms(f).into());
        }
        if let Some(t) = q.to {
            clauses.push("runs.started_at < ?");
            values.push(to_ms(t).into());
        }
        if let Some(s) = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            // LIKE metacharacters are escaped; the value is still bound as a parameter.
            let escaped = s
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            clauses.push(
                "(runs.title LIKE ? ESCAPE '\\' OR repositories.display_name LIKE ? ESCAPE '\\' OR runs.task_category LIKE ? ESCAPE '\\' OR runs.model LIKE ? ESCAPE '\\')",
            );
            let pat = format!("%{escaped}%");
            for _ in 0..4 {
                values.push(pat.clone().into());
            }
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let from_sql = "FROM runs LEFT JOIN repositories ON repositories.id = runs.repository_id";
        let total: u32 = self.conn.query_row(
            &format!("SELECT COUNT(*) {from_sql} {where_sql}"),
            rusqlite::params_from_iter(values.iter()),
            |r| r.get(0),
        )?;
        let limit = q.limit.unwrap_or(50).min(1000);
        let offset = q.offset.unwrap_or(0);
        let cols = RUN_COLUMNS
            .split(", ")
            .map(|c| format!("runs.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {cols} {from_sql} {where_sql} ORDER BY runs.started_at DESC LIMIT {limit} OFFSET {offset}"
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), run_from_row)?;
        let runs = rows.map(|r| r?).collect::<Result<Vec<_>>>()?;
        Ok((runs, total))
    }

    /// Terminal runs, oldest first, for estimator fitting and evaluation.
    pub fn finished_runs(&self, limit: u32) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM (SELECT * FROM runs WHERE status IN ('completed','failed','cancelled')
               ORDER BY ended_at DESC LIMIT ?1) ORDER BY ended_at ASC"
        ))?;
        let rows = stmt.query_map([limit], run_from_row)?;
        rows.map(|r| r?).collect()
    }

    /// Runs with any activity at or after `since`, in start order.
    pub fn runs_active_since(&self, since: DateTime<Utc>) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE COALESCE(ended_at, last_event_at) >= ?1 ORDER BY started_at"
        ))?;
        let rows = stmt.query_map([to_ms(since)], run_from_row)?;
        rows.map(|r| r?).collect()
    }

    /// Events for many runs at once, keyed by run id, each list in time order.
    pub fn events_for_runs(&self, run_ids: &[String]) -> Result<HashMap<String, Vec<EventRow>>> {
        let mut out: HashMap<String, Vec<EventRow>> = HashMap::new();
        for chunk in run_ids.chunks(400) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT {EVENT_COLUMNS} FROM events WHERE run_id IN ({marks})
                 ORDER BY run_id, source_timestamp, received_timestamp"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), event_from_row)?;
            for row in rows {
                let e = row??;
                if let Some(id) = e.run_id.clone() {
                    out.entry(id).or_default().push(e);
                }
            }
        }
        Ok(out)
    }

    pub fn insert_run_signals(&self, run_id: &str, active_ms: u64, s: &RunSignals) -> Result<()> {
        self.conn.execute(
            "INSERT INTO run_signals (run_id, active_ms, signals_json) VALUES (?1, ?2, ?3)",
            params![run_id, active_ms as i64, serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    pub fn signals_for_runs(
        &self,
        run_ids: &[String],
    ) -> Result<HashMap<String, Vec<SignalSnapshot>>> {
        let mut out: HashMap<String, Vec<SignalSnapshot>> = HashMap::new();
        for chunk in run_ids.chunks(400) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT run_id, active_ms, signals_json FROM run_signals WHERE run_id IN ({marks})
                 ORDER BY run_id, active_ms, id"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (id, ms, json) = row?;
                if let Ok(signals) = serde_json::from_str(&json) {
                    out.entry(id).or_default().push(SignalSnapshot {
                        active_ms: ms.max(0) as u64,
                        signals,
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn upsert_command_stat(&self, c: &CommandStatRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO command_stats (repository_id, command_key, samples, log_mean, log_m2, failures, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(repository_id, command_key) DO UPDATE SET samples = excluded.samples,
               log_mean = excluded.log_mean, log_m2 = excluded.log_m2, failures = excluded.failures,
               last_seen_at = excluded.last_seen_at",
            params![
                c.repository_id,
                c.command_key,
                c.samples,
                c.log_mean,
                c.log_m2,
                c.failures,
                to_ms(c.last_seen_at)
            ],
        )?;
        Ok(())
    }

    pub fn command_stats(&self) -> Result<Vec<CommandStatRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT repository_id, command_key, samples, log_mean, log_m2, failures, last_seen_at FROM command_stats",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CommandStatRow {
                repository_id: r.get(0)?,
                command_key: r.get(1)?,
                samples: r.get(2)?,
                log_mean: r.get(3)?,
                log_m2: r.get(4)?,
                failures: r.get(5)?,
                last_seen_at: from_ms(r.get(6)?),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_repository_file_count(&self, id: &str, files: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE repositories SET file_count = ?2 WHERE id = ?1",
            params![id, files],
        )?;
        Ok(())
    }

    pub fn repository_file_counts(&self) -> Result<HashMap<String, u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_count FROM repositories WHERE file_count IS NOT NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Removes stored prompt term vectors (used when similar-prompt lookup is turned off).
    pub fn clear_prompt_sketches(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE runs SET prompt_features_json = json_remove(prompt_features_json, '$.sketch')
             WHERE prompt_features_json IS NOT NULL AND json_type(prompt_features_json, '$.sketch') IS NOT NULL",
            [],
        )?)
    }

    pub fn count_runs(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))?)
    }

    // ---------------------------------------------------------------- events

    /// Inserts an event. Returns `false` when an event with the same id already
    /// exists (duplicate delivery).
    pub fn insert_event(&self, e: &EventRow) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO events (id, run_id, session_id, event_type, source_timestamp, received_timestamp,
                activity_category, tool_identifier, duration_ms, status, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                e.id,
                e.run_id,
                e.session_id,
                e.event_type.as_str(),
                to_ms(e.source_timestamp),
                to_ms(e.received_timestamp),
                e.activity.map(|a| a.as_str()),
                e.tool,
                e.duration_ms.map(|v| v as i64),
                e.status,
                serde_json::to_string(&e.metadata)?,
            ],
        )?;
        Ok(n == 1)
    }

    pub fn event_exists(&self, id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn events_for_run(&self, run_id: &str) -> Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {EVENT_COLUMNS} FROM events WHERE run_id = ?1 ORDER BY source_timestamp, received_timestamp"
        ))?;
        let rows = stmt.query_map([run_id], event_from_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn count_events(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    // ------------------------------------------------------------- forecasts

    pub fn insert_forecast(&self, f: &EtaForecast) -> Result<()> {
        let s = |v: Option<u32>| v.map(|x| x as i64 * 1000);
        self.conn.execute(
            "INSERT INTO forecasts (run_id, model_version, generated_at, active_elapsed_ms, remaining_p10_ms,
                remaining_p50_ms, remaining_p90_ms, confidence, forecast_status, basis, sample_size, reason_codes_json,
                raw_p10_ms, raw_p50_ms, raw_p90_ms, calibrated, components_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                f.run_id,
                f.model_version,
                to_ms(f.generated_at),
                f.active_elapsed_seconds as i64 * 1000,
                s(f.remaining_p10_seconds),
                s(f.remaining_p50_seconds),
                s(f.remaining_p90_seconds),
                f.confidence.as_str(),
                f.status.as_str(),
                basis_str(f.basis),
                f.sample_size as f64,
                serde_json::to_string(&f.reason_codes)?,
                s(f.raw_p10_seconds),
                s(f.raw_p50_seconds),
                s(f.raw_p90_seconds),
                f.calibrated,
                serde_json::to_string(&f.components)?,
            ],
        )?;
        Ok(())
    }

    pub fn forecasts_for_run(&self, run_id: &str) -> Result<Vec<EtaForecast>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FORECAST_COLUMNS} FROM forecasts WHERE run_id = ?1 ORDER BY generated_at, id"
        ))?;
        let rows = stmt.query_map([run_id], forecast_from_row)?;
        rows.map(|r| r?).collect()
    }

    /// All forecasts for terminal runs, grouped by run (for evaluation).
    pub fn forecasts_for_finished_runs(&self, limit_runs: u32) -> Result<Vec<EtaForecast>> {
        let cols = FORECAST_COLUMNS
            .split(", ")
            .map(|c| format!("f.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {cols} FROM forecasts f JOIN (
                SELECT id FROM runs WHERE status IN ('completed','failed','cancelled') ORDER BY ended_at DESC LIMIT ?1
             ) r ON r.id = f.run_id
             ORDER BY f.run_id, f.generated_at, f.id"
        ))?;
        let rows = stmt.query_map([limit_runs], forecast_from_row)?;
        rows.map(|r| r?).collect()
    }

    // ----------------------------------------------------------- preferences

    pub fn load_preferences(&self) -> Result<Preferences> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM user_preferences")?;
        let mut map = serde_json::Map::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (k, v) = row?;
            if let Ok(val) = serde_json::from_str(&v) {
                map.insert(k, val);
            }
        }
        // Unknown or malformed keys fall back to defaults instead of failing startup.
        let mut prefs = serde_json::to_value(Preferences::default())?;
        if let serde_json::Value::Object(obj) = &mut prefs {
            for (k, v) in map {
                if obj.contains_key(&k) {
                    obj.insert(k, v);
                }
            }
        }
        Ok(serde_json::from_value(prefs).unwrap_or_default())
    }

    pub fn save_preferences(&self, p: &Preferences) -> Result<()> {
        let now = to_ms(Utc::now());
        if let serde_json::Value::Object(obj) = serde_json::to_value(p)? {
            for (k, v) in obj {
                self.conn.execute(
                    "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![k, serde_json::to_string(&v)?, now],
                )?;
            }
        }
        Ok(())
    }

    // --------------------------------------------------------- notifications

    /// Records a notification. Returns `false` if this (run, type) was already
    /// delivered, which is how notifications are deduplicated.
    pub fn record_notification(&self, run_id: &str, kind: &str, at: DateTime<Utc>) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO notification_log (run_id, notification_type, delivered_at) VALUES (?1, ?2, ?3)",
            params![run_id, kind, to_ms(at)],
        )?;
        Ok(n == 1)
    }

    // ---------------------------------------------------------- integrations

    pub fn upsert_installation(&self, i: &InstallationRow) -> Result<()> {
        let now = to_ms(i.updated_at);
        self.conn.execute(
            "INSERT INTO integration_installations (agent_type, scope, config_location, installed_version,
                installation_status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(agent_type, scope, config_location) DO UPDATE SET installed_version = excluded.installed_version,
                installation_status = excluded.installation_status, updated_at = excluded.updated_at",
            params![i.agent.as_str(), i.scope, i.config_location, i.installed_version, i.status, now],
        )?;
        Ok(())
    }

    pub fn installations(&self) -> Result<Vec<InstallationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_type, scope, config_location, installed_version, installation_status, updated_at
             FROM integration_installations ORDER BY agent_type, scope",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (a, scope, loc, ver, status, up) = row?;
            out.push(InstallationRow {
                agent: parse_enum(
                    "integration_installations.agent_type",
                    a,
                    AgentSource::parse,
                )?,
                scope,
                config_location: loc,
                installed_version: ver,
                status,
                updated_at: from_ms(up),
            });
        }
        Ok(out)
    }

    pub fn last_event_at_for_agent(&self, agent: AgentSource) -> Result<Option<DateTime<Utc>>> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT MAX(last_seen_at) FROM sessions WHERE agent_id = ?1",
            [agent.as_str()],
            |r| r.get(0),
        )?;
        Ok(v.map(from_ms))
    }

    // ------------------------------------------------------------- retention

    /// Deletes runs (and, by cascade, their events and forecasts) that started
    /// before `before`. `None` deletes all history.
    pub fn delete_history(&mut self, before: Option<DateTime<Utc>>) -> Result<usize> {
        self.transaction(|tx| {
            let cutoff = before.map(to_ms).unwrap_or(i64::MAX);
            let n = tx.execute(
                "DELETE FROM runs WHERE started_at < ?1 AND status IN ('completed','failed','cancelled','unknown')",
                [cutoff],
            )?;
            tx.execute("DELETE FROM events WHERE run_id IS NULL AND received_timestamp < ?1", [cutoff])?;
            tx.execute("DELETE FROM notification_log WHERE delivered_at < ?1", [cutoff])?;
            tx.execute(
                "DELETE FROM sessions WHERE id NOT IN (SELECT DISTINCT session_id FROM runs) AND last_seen_at < ?1",
                [cutoff],
            )?;
            tx.execute(
                "DELETE FROM repositories WHERE id NOT IN (SELECT DISTINCT repository_id FROM runs WHERE repository_id IS NOT NULL)
                   AND id NOT IN (SELECT DISTINCT repository_id FROM sessions WHERE repository_id IS NOT NULL)",
                [],
            )?;
            Ok(n)
        })
    }

    pub fn vacuum(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
}

fn basis_str(b: ForecastBasis) -> &'static str {
    match b {
        ForecastBasis::ColdStart => "cold_start",
        ForecastBasis::Blended => "blended",
        ForecastBasis::History => "history",
    }
}

fn parse_basis(s: &str) -> Option<ForecastBasis> {
    match s {
        "cold_start" => Some(ForecastBasis::ColdStart),
        "blended" => Some(ForecastBasis::Blended),
        "history" => Some(ForecastBasis::History),
        _ => None,
    }
}

const RUN_COLUMNS: &str = "id, session_id, source_turn_id, agent_id, repository_id, task_category, prompt_features_json, title, model, started_at, ended_at, last_event_at, wall_duration_ms, active_duration_ms, waiting_duration_ms, status, exit_reason, current_activity, current_tool, tool_calls, tool_failures, test_runs, plan_total, plan_completed, initial_p10_ms, initial_p50_ms, initial_p90_ms, effort, permission_mode, signals_json";

const EVENT_COLUMNS: &str = "id, run_id, session_id, event_type, source_timestamp, received_timestamp, activity_category, tool_identifier, duration_ms, status, metadata_json";

const FORECAST_COLUMNS: &str = "run_id, model_version, generated_at, active_elapsed_ms, remaining_p10_ms, remaining_p50_ms, remaining_p90_ms, confidence, forecast_status, basis, sample_size, reason_codes_json, raw_p10_ms, raw_p50_ms, raw_p90_ms, calibrated, components_json";

fn event_from_row(r: &Row) -> rusqlite::Result<Result<EventRow>> {
    let et: String = r.get(3)?;
    let act: Option<String> = r.get(6)?;
    let dur: Option<i64> = r.get(8)?;
    let meta: String = r.get(10)?;
    let id: String = r.get(0)?;
    let run_id: Option<String> = r.get(1)?;
    let session_id: Option<String> = r.get(2)?;
    let st: i64 = r.get(4)?;
    let rt: i64 = r.get(5)?;
    let tool: Option<String> = r.get(7)?;
    let status: Option<String> = r.get(9)?;
    Ok((|| -> Result<EventRow> {
        Ok(EventRow {
            id,
            run_id,
            session_id,
            event_type: parse_enum("events.event_type", et, EventType::parse)?,
            source_timestamp: from_ms(st),
            received_timestamp: from_ms(rt),
            activity: act.as_deref().and_then(ActivityCategory::parse),
            tool,
            duration_ms: dur.map(|v| v.max(0) as u64),
            status,
            metadata: serde_json::from_str(&meta).unwrap_or(serde_json::Value::Null),
        })
    })())
}

fn run_from_row(r: &Row) -> rusqlite::Result<Result<RunRow>> {
    let agent: String = r.get(3)?;
    let category: String = r.get(5)?;
    let features: Option<String> = r.get(6)?;
    let status: String = r.get(15)?;
    let activity: Option<String> = r.get(17)?;
    let u = |v: Option<i64>| v.map(|x| x.max(0) as u64);
    let base = (|| -> Result<RunRow> {
        Ok(RunRow {
            id: r.get(0)?,
            session_id: r.get(1)?,
            source_turn_id: r.get(2)?,
            agent: parse_enum("runs.agent_id", agent, AgentSource::parse)?,
            repository_id: r.get(4)?,
            task_category: TaskCategory::parse(&category).unwrap_or(TaskCategory::Unknown),
            prompt_features: features.and_then(|f| serde_json::from_str(&f).ok()),
            title: r.get(7)?,
            model: r.get(8)?,
            started_at: from_ms(r.get(9)?),
            ended_at: r.get::<_, Option<i64>>(10)?.map(from_ms),
            last_event_at: from_ms(r.get(11)?),
            wall_duration_ms: u(r.get(12)?),
            active_duration_ms: u(r.get(13)?),
            waiting_duration_ms: u(r.get(14)?),
            status: parse_enum("runs.status", status, RunState::parse)?,
            exit_reason: r.get(16)?,
            current_activity: activity.as_deref().and_then(ActivityCategory::parse),
            current_tool: r.get(18)?,
            tool_calls: r.get(19)?,
            tool_failures: r.get(20)?,
            test_runs: r.get(21)?,
            plan_total: r.get(22)?,
            plan_completed: r.get(23)?,
            initial_p10_ms: u(r.get(24)?),
            initial_p50_ms: u(r.get(25)?),
            initial_p90_ms: u(r.get(26)?),
            effort: r.get(27)?,
            permission_mode: r.get(28)?,
            signals: r
                .get::<_, Option<String>>(29)?
                .and_then(|j| serde_json::from_str(&j).ok()),
        })
    })();
    Ok(base)
}

fn session_from_row(r: &Row) -> rusqlite::Result<Result<SessionRow>> {
    let agent: String = r.get(1)?;
    Ok((|| -> Result<SessionRow> {
        Ok(SessionRow {
            id: r.get(0)?,
            agent: parse_enum("sessions.agent_id", agent, AgentSource::parse)?,
            source_session_id: r.get(2)?,
            repository_id: r.get(3)?,
            started_at: from_ms(r.get(4)?),
            last_seen_at: from_ms(r.get(5)?),
            status: r.get(6)?,
        })
    })())
}

fn forecast_from_row(r: &Row) -> rusqlite::Result<Result<EtaForecast>> {
    let secs = |v: Option<i64>| v.map(|x| (x.max(0) / 1000) as u32);
    let generated_at = from_ms(r.get(2)?);
    let elapsed_ms: i64 = r.get(3)?;
    let p10 = secs(r.get(4)?);
    let p50 = secs(r.get(5)?);
    let p90 = secs(r.get(6)?);
    let confidence: String = r.get(7)?;
    let status: String = r.get(8)?;
    let basis: String = r.get(9)?;
    let sample: f64 = r.get(10)?;
    let reasons: String = r.get(11)?;
    let raw = [secs(r.get(12)?), secs(r.get(13)?), secs(r.get(14)?)];
    let calibrated: bool = r.get(15)?;
    let components: String = r.get(16)?;
    let run_id: String = r.get(0)?;
    let model_version: String = r.get(1)?;
    Ok((|| -> Result<EtaForecast> {
        Ok(EtaForecast {
            run_id,
            generated_at,
            remaining_p10_seconds: p10,
            remaining_p50_seconds: p50,
            remaining_p90_seconds: p90,
            expected_finish_at: p50.map(|s| generated_at + chrono::Duration::seconds(s as i64)),
            confidence: parse_enum("forecasts.confidence", confidence, Confidence::parse)?,
            status: parse_enum("forecasts.forecast_status", status, ForecastStatus::parse)?,
            reason_codes: serde_json::from_str(&reasons).unwrap_or_default(),
            model_version,
            basis: parse_enum("forecasts.basis", basis, parse_basis)?,
            sample_size: sample as f32,
            active_elapsed_seconds: (elapsed_ms.max(0) / 1000) as u32,
            raw_p10_seconds: raw[0],
            raw_p50_seconds: raw[1],
            raw_p90_seconds: raw[2],
            calibrated,
            components: serde_json::from_str(&components).unwrap_or_default(),
        })
    })())
}

pub fn complexity_of(f: &Option<PromptFeatures>) -> Option<ComplexityBucket> {
    f.as_ref().map(|f| f.complexity)
}

#[cfg(test)]
mod tests;
