//! Integration status shared by the CLI and the desktop app: combines adapter
//! detection and verification with the collector's record of received events.

use crate::Client;
use chrono::{DateTime, Utc};
use runmarq_adapters::{binary, status, Adapter, InstallOptions, Scope};
use runmarq_core::api::IntegrationStatus;
use runmarq_core::event::AgentSource;
use runmarq_core::paths::Paths;
use runmarq_storage::{InstallationRow, Store};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub type LastEvents = BTreeMap<String, Option<DateTime<Utc>>>;

pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn options(paths: &Paths, scope: Scope, executable: PathBuf) -> InstallOptions {
    InstallOptions {
        scope,
        executable,
        backups_dir: paths.backups_dir(),
        home: None,
    }
}

fn open_store(paths: &Paths) -> Option<Store> {
    std::fs::create_dir_all(&paths.root).ok()?;
    Store::open(&paths.database()).ok()
}

/// Last event per agent, from the running collector or, failing that, the database.
pub fn last_events(paths: &Paths, client: Option<&Client>) -> LastEvents {
    let owned;
    let client = match client {
        Some(c) => Some(c),
        None => {
            owned = Client::discover(paths, Duration::from_secs(2)).ok();
            owned.as_ref()
        }
    };
    if let Some(c) = client {
        if let Ok(m) = c.get::<LastEvents>("/v1/agents/activity") {
            return m;
        }
    }
    let mut out = BTreeMap::new();
    if let Some(s) = open_store(paths) {
        for a in AgentSource::ALL {
            out.insert(
                a.as_str().to_string(),
                s.last_event_at_for_agent(a).ok().flatten(),
            );
        }
    }
    out
}

fn installed_at(store: Option<&Store>, agent: AgentSource) -> Option<DateTime<Utc>> {
    store?
        .installations()
        .ok()?
        .into_iter()
        .filter(|i| i.agent == agent && i.status == "installed")
        .map(|i| i.updated_at)
        .max()
}

pub fn status_for(
    paths: &Paths,
    adapter: &dyn Adapter,
    scope: &Scope,
    last: &LastEvents,
) -> IntegrationStatus {
    let store = open_store(paths);
    let opts = options(paths, scope.clone(), binary::installed_path(&paths.root));
    let det = adapter.detect(&home_dir());
    let ver = adapter.verify(&opts);
    let event = last.get(adapter.source().as_str()).copied().flatten();
    status::compute(
        adapter,
        &opts,
        &det,
        &ver,
        event,
        installed_at(store.as_ref(), adapter.source()),
    )
}

pub fn statuses(paths: &Paths, scope: &Scope, last: &LastEvents) -> Vec<IntegrationStatus> {
    runmarq_adapters::all()
        .iter()
        .map(|a| status_for(paths, a.as_ref(), scope, last))
        .collect()
}

pub fn record_installation(
    paths: &Paths,
    adapter: &dyn Adapter,
    scope: &Scope,
    config: &Path,
    state: &str,
) {
    if let Some(store) = open_store(paths) {
        let version = adapter.detect(&home_dir()).version;
        let _ = store.upsert_installation(&InstallationRow {
            agent: adapter.source(),
            scope: scope.label(),
            config_location: config.display().to_string(),
            installed_version: version,
            status: state.into(),
            updated_at: Utc::now(),
        });
    }
}
