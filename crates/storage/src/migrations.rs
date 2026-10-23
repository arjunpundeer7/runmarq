//! Versioned schema migrations. Migrations are append-only: never edit a
//! released migration, add a new one.
//!
//! All timestamps are stored as INTEGER milliseconds since the Unix epoch (UTC).

pub const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        r#"
CREATE TABLE agents (
    id                  TEXT PRIMARY KEY,
    type                TEXT NOT NULL,
    display_name        TEXT NOT NULL,
    installed_version   TEXT,
    integration_status  TEXT NOT NULL DEFAULT 'not_connected',
    capabilities_json   TEXT NOT NULL DEFAULT '{}',
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE TABLE repositories (
    id                TEXT PRIMARY KEY,
    local_identifier  TEXT NOT NULL UNIQUE,
    display_name      TEXT,
    first_seen_at     INTEGER NOT NULL,
    last_seen_at      INTEGER NOT NULL
);

CREATE TABLE sessions (
    id                 TEXT PRIMARY KEY,
    agent_id           TEXT NOT NULL,
    source_session_id  TEXT NOT NULL,
    repository_id      TEXT REFERENCES repositories(id) ON DELETE SET NULL,
    started_at         INTEGER NOT NULL,
    last_seen_at       INTEGER NOT NULL,
    status             TEXT NOT NULL DEFAULT 'active',
    UNIQUE (agent_id, source_session_id)
);

CREATE TABLE runs (
    id                          TEXT PRIMARY KEY,
    session_id                  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source_turn_id              TEXT,
    agent_id                    TEXT NOT NULL,
    repository_id               TEXT REFERENCES repositories(id) ON DELETE SET NULL,
    task_category               TEXT NOT NULL DEFAULT 'unknown',
    complexity_bucket           TEXT,
    prompt_features_json        TEXT,
    prompt_fingerprint          TEXT,
    title                       TEXT,
    model                       TEXT,
    started_at                  INTEGER NOT NULL,
    ended_at                    INTEGER,
    last_event_at               INTEGER NOT NULL,
    wall_duration_ms            INTEGER,
    active_duration_ms          INTEGER,
    waiting_duration_ms         INTEGER,
    status                      TEXT NOT NULL,
    exit_reason                 TEXT,
    current_activity            TEXT,
    current_tool                TEXT,
    tool_calls                  INTEGER NOT NULL DEFAULT 0,
    tool_failures               INTEGER NOT NULL DEFAULT 0,
    test_runs                   INTEGER NOT NULL DEFAULT 0,
    plan_total                  INTEGER,
    plan_completed              INTEGER,
    initial_p10_ms              INTEGER,
    initial_p50_ms              INTEGER,
    initial_p90_ms              INTEGER,
    created_at                  INTEGER NOT NULL,
    updated_at                  INTEGER NOT NULL
);
CREATE INDEX idx_runs_started ON runs(started_at DESC);
CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_runs_agent ON runs(agent_id, started_at DESC);
CREATE INDEX idx_runs_repo ON runs(repository_id, started_at DESC);
CREATE INDEX idx_runs_session ON runs(session_id, started_at DESC);
CREATE UNIQUE INDEX idx_runs_turn ON runs(session_id, source_turn_id) WHERE source_turn_id IS NOT NULL;

CREATE TABLE events (
    id                  TEXT PRIMARY KEY,
    run_id              TEXT REFERENCES runs(id) ON DELETE CASCADE,
    session_id          TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    event_type          TEXT NOT NULL,
    source_timestamp    INTEGER NOT NULL,
    received_timestamp  INTEGER NOT NULL,
    activity_category   TEXT,
    tool_identifier     TEXT,
    duration_ms         INTEGER,
    status              TEXT,
    metadata_json       TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_events_run ON events(run_id, source_timestamp);
CREATE INDEX idx_events_received ON events(received_timestamp);

CREATE TABLE forecasts (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id                 TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    model_version          TEXT NOT NULL,
    generated_at           INTEGER NOT NULL,
    active_elapsed_ms      INTEGER NOT NULL,
    remaining_p10_ms       INTEGER,
    remaining_p50_ms       INTEGER,
    remaining_p90_ms       INTEGER,
    confidence             TEXT NOT NULL,
    forecast_status        TEXT NOT NULL,
    basis                  TEXT NOT NULL,
    sample_size            REAL NOT NULL DEFAULT 0,
    reason_codes_json      TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX idx_forecasts_run ON forecasts(run_id, generated_at);

CREATE TABLE integration_installations (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_type           TEXT NOT NULL,
    scope                TEXT NOT NULL,
    config_location      TEXT NOT NULL,
    installed_version    TEXT,
    installation_status  TEXT NOT NULL,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    UNIQUE (agent_type, scope, config_location)
);

CREATE TABLE user_preferences (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE notification_log (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id             TEXT NOT NULL,
    notification_type  TEXT NOT NULL,
    delivered_at       INTEGER NOT NULL,
    UNIQUE (run_id, notification_type)
);
"#,
    ),
    (
        2,
        r#"
ALTER TABLE runs ADD COLUMN effort TEXT;
ALTER TABLE runs ADD COLUMN permission_mode TEXT;
ALTER TABLE runs ADD COLUMN signals_json TEXT;

ALTER TABLE forecasts ADD COLUMN raw_p10_ms INTEGER;
ALTER TABLE forecasts ADD COLUMN raw_p50_ms INTEGER;
ALTER TABLE forecasts ADD COLUMN raw_p90_ms INTEGER;
ALTER TABLE forecasts ADD COLUMN calibrated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE forecasts ADD COLUMN components_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE repositories ADD COLUMN file_count INTEGER;

CREATE TABLE command_stats (
    repository_id  TEXT NOT NULL DEFAULT '',
    command_key    TEXT NOT NULL,
    samples        INTEGER NOT NULL,
    log_mean       REAL NOT NULL,
    log_m2         REAL NOT NULL,
    failures       INTEGER NOT NULL DEFAULT 0,
    last_seen_at   INTEGER NOT NULL,
    PRIMARY KEY (repository_id, command_key)
);

CREATE TABLE run_signals (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id        TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    active_ms     INTEGER NOT NULL,
    signals_json  TEXT NOT NULL
);
CREATE INDEX idx_run_signals_run ON run_signals(run_id, active_ms);
"#,
    ),
];
