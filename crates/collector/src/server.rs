//! Local HTTP API.
//!
//! Security model:
//! - Bound to 127.0.0.1 only, on an ephemeral port.
//! - Every route requires `Authorization: Bearer <token>`; the token is
//!   compared in constant time and is only available to processes that can
//!   read the user's data directory.
//! - Requests carrying an `Origin` header are rejected outright: runmarq's
//!   own clients never send one, while every cross-site browser request does.
//!   CORS is never enabled, so pre-flighted requests fail as well.
//! - The `Host` header must name the loopback address, defeating DNS rebinding.
//! - Bodies are size-limited; POST/PUT require `application/json`.

use crate::engine::{Engine, TrainingMode};
use crate::lifecycle::{self, InstanceLock};
use crate::{learning, probe, spool, usage};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use futures::stream::Stream;
use runmarq_core::api::{
    Diagnostics, Discovery, Health, HistoryQuery, IngestRequest, IngestResponse, Preferences,
    StreamMessage,
};
use runmarq_core::event::SCHEMA_VERSION;
use runmarq_core::paths::Paths;
use runmarq_storage::Store;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Notify};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_EVENTS_PER_REQUEST: usize = 500;

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub tx: broadcast::Sender<StreamMessage>,
    pub token: String,
    pub port: u16,
    pub started_at: DateTime<Utc>,
    pub paths: Paths,
    pub shutdown: Notify,
    pub subscribers: AtomicU32,
    pub spool_replayed: AtomicU64,
}

type Shared = Arc<AppState>;

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn guard(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let h = req.headers();
    if h.contains_key(header::ORIGIN) {
        return (
            StatusCode::FORBIDDEN,
            "cross-origin requests are not allowed",
        )
            .into_response();
    }
    let host_ok = h
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v == format!("127.0.0.1:{}", s.port) || v == format!("localhost:{}", s.port)
        });
    if !host_ok {
        return (StatusCode::FORBIDDEN, "unexpected host").into_response();
    }
    let authed = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| constant_time_eq(t.as_bytes(), s.token.as_bytes()));
    if !authed {
        return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
    }
    if matches!(*req.method(), Method::POST | Method::PUT) {
        let json = h
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
            });
        if !json {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "expected application/json",
            )
                .into_response();
        }
    }
    if let Some(len) = h
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
    {
        if len > MAX_BODY_BYTES {
            return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
        }
    }
    next.run(req).await
}

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<crate::engine::EngineError> for ApiError {
    fn from(e: crate::engine::EngineError) -> Self {
        match e {
            crate::engine::EngineError::Invalid(m) => ApiError(StatusCode::BAD_REQUEST, m),
            other => ApiError(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

/// Runs a closure against the engine on a blocking thread.
async fn with_engine<T: Send + 'static>(
    s: &Shared,
    f: impl FnOnce(&mut Engine) -> T + Send + 'static,
) -> Result<T, ApiError> {
    let s = s.clone();
    tokio::task::spawn_blocking(move || {
        let mut e = s.engine.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut e)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn broadcast_all(s: &Shared, msgs: Vec<StreamMessage>) {
    for m in msgs {
        let _ = s.tx.send(m);
    }
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        version: VERSION.into(),
        schema_version: SCHEMA_VERSION,
        pid: std::process::id(),
    })
}

async fn ingest(
    State(s): State<Shared>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    if req.events.len() > MAX_EVENTS_PER_REQUEST {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too many events".into(),
        ));
    }
    let shared = s.clone();
    let (resp, msgs) = with_engine(&s, move |e| {
        let now = Utc::now();
        if let Some(ms) = req.hook_latency_ms {
            e.stats.hook_latency.push(ms);
        }
        let mut resp = IngestResponse {
            accepted: 0,
            duplicates: 0,
            rejected: 0,
        };
        let mut msgs = Vec::new();
        for env in req.events {
            if req.replayed {
                e.stats.replayed += 1;
            }
            match e.ingest(env, now) {
                Ok(o) if o.duplicate => resp.duplicates += 1,
                Ok(o) => {
                    resp.accepted += 1;
                    msgs.extend(o.messages);
                }
                Err(_) => resp.rejected += 1,
            }
        }
        (resp, msgs)
    })
    .await?;
    broadcast_all(&shared, msgs);
    Ok(Json(resp))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LauncherExit {
    launcher_id: String,
}

async fn launcher_exit(
    State(s): State<Shared>,
    Json(body): Json<LauncherExit>,
) -> Result<StatusCode, ApiError> {
    let msgs = with_engine(&s, move |e| {
        e.launcher_exited(&body.launcher_id, Utc::now())
    })
    .await??;
    broadcast_all(&s, msgs);
    Ok(StatusCode::NO_CONTENT)
}

async fn active(State(s): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(with_engine(&s, |e| e.active_runs(Utc::now())).await?))
}

async fn history(
    State(s): State<Shared>,
    Query(q): Query<HistoryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        with_engine(&s, move |e| e.history(&q, Utc::now())).await??,
    ))
}

async fn run_detail(State(s): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    match with_engine(&s, move |e| e.run_detail(&id, Utc::now())).await?? {
        Some(d) => Ok(Json(d).into_response()),
        None => Err(ApiError(StatusCode::NOT_FOUND, "run not found".into())),
    }
}

async fn repositories(State(s): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    let r = with_engine(&s, |e| e.store().list_repositories()).await?;
    r.map(Json)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn insights(State(s): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(with_engine(&s, |e| e.insights()).await??))
}

async fn usage_report(State(s): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(
        with_engine(&s, |e| usage::report(e, Utc::now())).await??,
    ))
}

async fn agent_activity(State(s): State<Shared>) -> Result<impl IntoResponse, ApiError> {
    let map = with_engine(&s, |e| {
        runmarq_core::event::AgentSource::ALL
            .iter()
            .map(|a| {
                (
                    a.as_str().to_string(),
                    e.store().last_event_at_for_agent(*a).ok().flatten(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    })
    .await?;
    Ok(Json(map))
}

async fn diagnostics(State(s): State<Shared>) -> Result<Json<Diagnostics>, ApiError> {
    let shared = s.clone();
    let d = with_engine(&s, move |e| Diagnostics {
        collector_version: VERSION.into(),
        schema_version: SCHEMA_VERSION,
        pid: std::process::id(),
        started_at: shared.started_at,
        uptime_seconds: (Utc::now() - shared.started_at).num_seconds().max(0) as u64,
        data_dir: shared.paths.root.display().to_string(),
        database_bytes: std::fs::metadata(shared.paths.database())
            .map(|m| m.len())
            .unwrap_or(0),
        events_received: e.stats.received,
        events_rejected: e.stats.rejected,
        events_duplicate: e.stats.duplicates,
        spooled_events_replayed: shared.spool_replayed.load(Ordering::Relaxed),
        spool_pending: spool::read_all(&shared.paths)
            .iter()
            .map(|(_, ev)| ev.len() as u64)
            .sum(),
        subscribers: shared.subscribers.load(Ordering::Relaxed),
        open_runs: e.open_run_count() as u32,
        ingest_latency: e.stats.ingest_latency.stats(),
        forecast_latency: e.stats.forecast_latency.stats(),
        db_write_latency: e.stats.db_latency.stats(),
        hook_latency: e.stats.hook_latency.stats(),
        training_latency: e.stats.training_ms.stats(),
        recent_errors: e.stats.recent_errors.iter().cloned().collect(),
    })
    .await?;
    Ok(Json(d))
}

async fn get_prefs(State(s): State<Shared>) -> Result<Json<Preferences>, ApiError> {
    Ok(Json(with_engine(&s, |e| e.preferences().clone()).await?))
}

async fn put_prefs(
    State(s): State<Shared>,
    Json(p): Json<Preferences>,
) -> Result<Json<Preferences>, ApiError> {
    let msg = with_engine(&s, move |e| e.set_preferences(p)).await??;
    let out = match &msg {
        StreamMessage::PreferencesChanged { preferences } => preferences.clone(),
        _ => unreachable!(),
    };
    let _ = s.tx.send(msg);
    Ok(Json(out))
}

#[derive(Deserialize)]
struct DeleteQuery {
    before: Option<DateTime<Utc>>,
}

async fn delete_history(
    State(s): State<Shared>,
    Query(q): Query<DeleteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let n = with_engine(&s, move |e| e.delete_history(q.before)).await??;
    Ok(Json(serde_json::json!({ "deleted": n })))
}

fn csv_field(s: &str) -> String {
    // Neutralize spreadsheet formula injection and quote every field.
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    format!("\"{}\"", s.replace('"', "\"\""))
}

async fn export_csv(
    State(s): State<Shared>,
    Query(mut q): Query<HistoryQuery>,
) -> Result<Response, ApiError> {
    q.limit = Some(q.limit.unwrap_or(1000).min(1000));
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let mut page_q = q.clone();
        page_q.offset = Some(offset);
        let page = with_engine(&s, move |e| e.history(&page_q, Utc::now())).await??;
        let n = page.runs.len() as u32;
        all.extend(page.runs);
        offset += n;
        if n == 0 || offset >= page.total || offset >= 50_000 {
            break;
        }
    }
    let mut out = String::from(
        "run_id,agent,repository,task_category,status,exit_reason,started_at,ended_at,wall_seconds,active_seconds,waiting_seconds,tool_calls,tool_failures,initial_p10_seconds,initial_p50_seconds,initial_p90_seconds\n",
    );
    for r in all {
        let row = [
            r.id,
            r.agent.as_str().into(),
            r.repository_name.unwrap_or_default(),
            r.task_category.as_str().into(),
            r.state.as_str().into(),
            r.exit_reason.unwrap_or_default(),
            r.started_at.to_rfc3339(),
            r.ended_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            (r.wall_ms / 1000).to_string(),
            (r.active_ms / 1000).to_string(),
            (r.waiting_ms / 1000).to_string(),
            r.tool_calls.to_string(),
            r.tool_failures.to_string(),
            r.initial_p10_seconds
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.initial_p50_seconds
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.initial_p90_seconds
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ];
        out.push_str(
            &row.iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out).into_response())
}

/// Decrements the subscriber count when an SSE client disconnects.
struct SubscriberGuard(Shared);

impl Drop for SubscriberGuard {
    fn drop(&mut self) {
        self.0.subscribers.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn stream(State(s): State<Shared>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use futures::StreamExt;
    let rx = s.tx.subscribe();
    let snapshot = with_engine(&s, |e| e.snapshot(Utc::now())).await.ok();
    s.subscribers.fetch_add(1, Ordering::Relaxed);
    let guard = SubscriberGuard(s.clone());
    let first = futures::stream::iter(snapshot.into_iter().map(|m| Ok(to_event(&m))));
    // Lagged receivers skip missed messages; clients resync from the next snapshot-worthy update.
    let rest = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|r| async move { r.ok().map(|m| Ok::<_, Infallible>(to_event(&m))) });
    let out = first.chain(rest).map(move |e| {
        let _keep = &guard;
        e
    });
    Sse::new(out).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

fn to_event(m: &StreamMessage) -> Event {
    Event::default().data(serde_json::to_string(m).unwrap_or_else(|_| "{}".into()))
}

async fn shutdown(State(s): State<Shared>) -> StatusCode {
    s.shutdown.notify_waiters();
    StatusCode::ACCEPTED
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/events", post(ingest))
        .route("/v1/launcher/exit", post(launcher_exit))
        .route("/v1/runs/active", get(active))
        .route("/v1/runs", get(history))
        .route("/v1/runs/{id}", get(run_detail))
        .route("/v1/repositories", get(repositories))
        .route("/v1/insights", get(insights))
        .route("/v1/usage", get(usage_report))
        .route("/v1/agents/activity", get(agent_activity))
        .route("/v1/diagnostics", get(diagnostics))
        .route("/v1/preferences", get(get_prefs).put(put_prefs))
        .route("/v1/history", delete(delete_history))
        .route("/v1/export.csv", get(export_csv))
        .route("/v1/stream", get(stream))
        .route("/v1/shutdown", post(shutdown))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Options for running a collector.
pub struct CollectorOptions {
    pub paths: Paths,
}

/// A running collector: the bound address plus a handle to stop it.
pub struct Running {
    pub state: Shared,
    pub port: u16,
    pub join: tokio::task::JoinHandle<()>,
    _lock: InstanceLock,
}

impl Running {
    pub fn stop(&self) {
        self.state.shutdown.notify_waiters();
    }
}

fn drain_spool(state: &Shared) {
    let files = spool::read_all(&state.paths);
    if files.is_empty() {
        return;
    }
    let mut e = state.engine.lock().unwrap_or_else(|p| p.into_inner());
    let mut msgs = Vec::new();
    for (path, events) in files {
        for env in events {
            e.stats.replayed += 1;
            state.spool_replayed.fetch_add(1, Ordering::Relaxed);
            if let Ok(o) = e.ingest(env, Utc::now()) {
                msgs.extend(o.messages);
            }
        }
        spool::remove(&path);
    }
    drop(e);
    broadcast_all(state, msgs);
}

/// Reads transcripts and samples `git` for open runs without holding the
/// engine lock, then hands the results back.
fn run_probes(state: &Shared) {
    let requests = {
        let mut e = state.engine.lock().unwrap_or_else(|p| p.into_inner());
        e.probe_requests(Utc::now())
    };
    if requests.is_empty() {
        return;
    }
    let results: Vec<probe::ProbeResult> = requests.into_iter().map(probe::run).collect();
    let msgs = {
        let mut e = state.engine.lock().unwrap_or_else(|p| p.into_inner());
        let now = Utc::now();
        results
            .into_iter()
            .flat_map(|r| e.apply_probe(r, now))
            .collect()
    };
    broadcast_all(state, msgs);
}

/// Refits the models on a separate database connection when enough has
/// changed. Forecasts keep using the previous fit until the new one is ready.
fn start_training(state: &Shared) {
    let generation = {
        let mut e = state.engine.lock().unwrap_or_else(|p| p.into_inner());
        if !e.training_due(Utc::now()) {
            return;
        }
        e.begin_training()
    };
    let s = state.clone();
    tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let result = learning::train(&s.paths.database(), Utc::now());
        let mut e = s.engine.lock().unwrap_or_else(|p| p.into_inner());
        match result {
            Ok(est) => {
                e.stats
                    .training_ms
                    .push(started.elapsed().as_secs_f64() * 1000.0);
                tracing::info!(
                    runs = est.report().finished_runs,
                    calibrated = est.is_calibrated(),
                    ms = started.elapsed().as_millis() as u64,
                    "models refitted"
                );
                e.install_estimator(est, generation, Utc::now());
            }
            Err(err) => e.training_failed(&err.to_string()),
        }
    });
}

/// Starts the collector inside the current Tokio runtime.
pub async fn start(opts: CollectorOptions) -> anyhow::Result<Running> {
    let paths = opts.paths;
    lifecycle::ensure_dirs(&paths)?;
    let lock = lifecycle::acquire_lock(&paths)?;
    let store = Store::open(&paths.database())?;
    let mut engine = Engine::new(store)?;
    engine.set_training_mode(TrainingMode::Background);
    engine.set_hook_config_path(paths.hook_config());
    let recovered = engine.recover(Utc::now())?;
    if !recovered.is_empty() {
        tracing::info!(
            count = recovered.len(),
            "recovered open runs as status unknown"
        );
    }
    if let Err(e) = engine.apply_retention(Utc::now()) {
        tracing::warn!("retention cleanup failed: {e}");
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = lifecycle::generate_token();
    let (tx, _) = broadcast::channel(1024);
    let state = Arc::new(AppState {
        engine: Mutex::new(engine),
        tx,
        token: token.clone(),
        port,
        started_at: Utc::now(),
        paths: paths.clone(),
        shutdown: Notify::new(),
        subscribers: AtomicU32::new(0),
        spool_replayed: AtomicU64::new(0),
    });
    lifecycle::write_discovery(
        &paths,
        &Discovery {
            pid: std::process::id(),
            port,
            token,
            version: VERSION.into(),
            started_at: state.started_at,
        },
    )?;
    tracing::info!(port, "collector listening on 127.0.0.1");

    // Replay anything hooks spooled while no collector was running.
    {
        let s = state.clone();
        tokio::task::spawn_blocking(move || drain_spool(&s)).await?;
    }

    // Maintenance loop: staleness, forecasts, spool, retention.
    {
        let s = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            let mut n: u64 = 0;
            loop {
                tick.tick().await;
                n += 1;
                let s2 = s.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let msgs = {
                        let mut e = s2.engine.lock().unwrap_or_else(|p| p.into_inner());
                        let msgs = e.tick(Utc::now());
                        if n.is_multiple_of(120) {
                            e.refresh_calibration();
                        }
                        if n.is_multiple_of(4320) {
                            let _ = e.apply_retention(Utc::now());
                        }
                        msgs
                    };
                    broadcast_all(&s2, msgs);
                    if n.is_multiple_of(2) {
                        drain_spool(&s2);
                    }
                    run_probes(&s2);
                    start_training(&s2);
                })
                .await;
            }
        });
    }

    let app = router(state.clone());
    let s = state.clone();
    let join = tokio::spawn(async move {
        let shutdown = async move {
            s.shutdown.notified().await;
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
        {
            tracing::error!("server error: {e}");
        }
    });
    Ok(Running {
        state,
        port,
        join,
        _lock: lock,
    })
}

/// Runs a collector until it is asked to stop or receives Ctrl+C.
pub async fn run_until_shutdown(opts: CollectorOptions) -> anyhow::Result<()> {
    let paths = opts.paths.clone();
    let running = start(opts).await?;
    let state = running.state.clone();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("interrupt received; shutting down");
            running.stop();
        }
        _ = state.shutdown.notified() => {}
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), running.join).await;
    lifecycle::remove_discovery(&paths, std::process::id());
    {
        let e = state.engine.lock().unwrap_or_else(|p| p.into_inner());
        let _ = e
            .store()
            .conn()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
    tracing::info!("collector stopped");
    Ok(())
}
