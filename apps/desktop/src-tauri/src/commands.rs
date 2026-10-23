//! Commands invoked by the webview. All collector access happens here, on a
//! blocking thread, so the token stays in the native process.

use crate::AppState;
use runmarq_adapters::{
    binary, for_source, shell_path, InstallPlan, InstallResult, Scope, UninstallResult,
};
use runmarq_collector::integrations as integ;
use runmarq_core::api::{
    Diagnostics, HistoryPage, HistoryQuery, Insights, IntegrationStatus, Preferences,
    RepositoryView, RunDetail, RunView, UsageReport,
};
use runmarq_core::event::AgentSource;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type Shared<'a> = State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn history_path(q: &HistoryQuery) -> String {
    let mut parts = Vec::new();
    if let Some(a) = q.agent {
        parts.push(format!("agent={}", a.as_str()));
    }
    if let Some(r) = &q.repository_id {
        parts.push(format!("repositoryId={}", encode(r)));
    }
    if let Some(s) = q.state {
        parts.push(format!("state={}", s.as_str()));
    }
    if let Some(f) = q.from {
        parts.push(format!("from={}", encode(&f.to_rfc3339())));
    }
    if let Some(t) = q.to {
        parts.push(format!("to={}", encode(&t.to_rfc3339())));
    }
    if let Some(s) = q.search.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("search={}", encode(s)));
    }
    if let Some(l) = q.limit {
        parts.push(format!("limit={l}"));
    }
    if let Some(o) = q.offset {
        parts.push(format!("offset={o}"));
    }
    format!("/v1/runs?{}", parts.join("&"))
}

fn scope_of(project_root: Option<String>) -> CmdResult<Scope> {
    match project_root {
        Some(p) => {
            let root = std::fs::canonicalize(&p).map_err(|e| format!("{p}: {e}"))?;
            if !root.is_dir() {
                return Err(format!("{p} is not a folder"));
            }
            Ok(Scope::Project { root })
        }
        None => Ok(Scope::User),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    platform: String,
    data_dir: String,
    logs_dir: String,
    hook_executable: String,
    collector: crate::collector::CollectorInfo,
}

#[tauri::command]
pub async fn app_info(state: Shared<'_>) -> CmdResult<AppInfo> {
    let s = state.inner().clone();
    blocking(move || {
        let c = &s.collector;
        Ok(AppInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            data_dir: c.paths.root.display().to_string(),
            logs_dir: c.paths.logs_dir().display().to_string(),
            hook_executable: binary::installed_path(&c.paths.root).display().to_string(),
            collector: c.info(),
        })
    })
    .await
}

#[tauri::command]
pub async fn active_runs(state: Shared<'_>) -> CmdResult<Vec<RunView>> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/runs/active"))).await
}

#[tauri::command]
pub async fn history(state: Shared<'_>, query: HistoryQuery) -> CmdResult<HistoryPage> {
    let s = state.inner().clone();
    let path = history_path(&query);
    blocking(move || s.collector.call(|c| c.get(&path))).await
}

#[tauri::command]
pub async fn run_detail(state: Shared<'_>, id: String) -> CmdResult<Option<RunDetail>> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || id.len() > 64 {
        return Err("invalid run id".into());
    }
    let s = state.inner().clone();
    blocking(move || {
        match s
            .collector
            .call(|c| c.get::<RunDetail>(&format!("/v1/runs/{id}")))
        {
            Ok(d) => Ok(Some(d)),
            Err(e) if e.contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    })
    .await
}

#[tauri::command]
pub async fn repositories(state: Shared<'_>) -> CmdResult<Vec<RepositoryView>> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/repositories"))).await
}

#[tauri::command]
pub async fn insights(state: Shared<'_>) -> CmdResult<Insights> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/insights"))).await
}

#[tauri::command]
pub async fn usage(state: Shared<'_>) -> CmdResult<UsageReport> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/usage"))).await
}

#[tauri::command]
pub async fn diagnostics(state: Shared<'_>) -> CmdResult<Diagnostics> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/diagnostics"))).await
}

#[tauri::command]
pub async fn get_preferences(state: Shared<'_>) -> CmdResult<Preferences> {
    let s = state.inner().clone();
    blocking(move || s.collector.call(|c| c.get("/v1/preferences"))).await
}

#[tauri::command]
pub async fn set_preferences(
    app: AppHandle,
    state: Shared<'_>,
    preferences: Preferences,
) -> CmdResult<Preferences> {
    let s = state.inner().clone();
    let saved: Preferences = blocking(move || {
        s.collector
            .call(|c| c.send("PUT", "/v1/preferences", &preferences))
    })
    .await?;
    crate::apply_preferences(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub async fn integrations(
    state: Shared<'_>,
    project_root: Option<String>,
) -> CmdResult<Vec<IntegrationStatus>> {
    let s = state.inner().clone();
    blocking(move || {
        let scope = scope_of(project_root)?;
        let client = s.collector.call(|c| Ok(c.clone())).ok();
        let last = integ::last_events(&s.collector.paths, client.as_ref());
        Ok(integ::statuses(&s.collector.paths, &scope, &last))
    })
    .await
}

fn hook_exe(s: &AppState) -> CmdResult<PathBuf> {
    s.collector.prepare_cli()
}

#[tauri::command]
pub async fn integration_plan(
    state: Shared<'_>,
    agent: AgentSource,
    project_root: Option<String>,
) -> CmdResult<InstallPlan> {
    let s = state.inner().clone();
    blocking(move || {
        let scope = scope_of(project_root)?;
        let opts = integ::options(&s.collector.paths, scope, hook_exe(&s)?);
        for_source(agent).plan(&opts).map_err(|e| e.to_string())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    result: InstallResult,
    status: IntegrationStatus,
}

#[tauri::command]
pub async fn integration_install(
    state: Shared<'_>,
    agent: AgentSource,
    project_root: Option<String>,
) -> CmdResult<InstallOutcome> {
    let s = state.inner().clone();
    blocking(move || {
        let scope = scope_of(project_root)?;
        let paths = &s.collector.paths;
        let opts = integ::options(paths, scope.clone(), hook_exe(&s)?);
        let adapter = for_source(agent);
        let result = adapter.install(&opts).map_err(|e| e.to_string())?;
        integ::record_installation(
            paths,
            adapter.as_ref(),
            &scope,
            &result.config_path,
            "installed",
        );
        let client = s.collector.call(|c| Ok(c.clone())).ok();
        let last = integ::last_events(paths, client.as_ref());
        let status = integ::status_for(paths, adapter.as_ref(), &scope, &last);
        Ok(InstallOutcome { result, status })
    })
    .await
}

#[tauri::command]
pub async fn integration_remove(
    state: Shared<'_>,
    agent: AgentSource,
    project_root: Option<String>,
) -> CmdResult<UninstallResult> {
    let s = state.inner().clone();
    blocking(move || {
        let scope = scope_of(project_root)?;
        let paths = &s.collector.paths;
        let opts = integ::options(paths, scope.clone(), binary::installed_path(&paths.root));
        let adapter = for_source(agent);
        let r = adapter.uninstall(&opts).map_err(|e| e.to_string())?;
        integ::record_installation(paths, adapter.as_ref(), &scope, &r.config_path, "removed");
        Ok(r)
    })
    .await
}

#[tauri::command]
pub async fn export_csv(app: AppHandle, state: Shared<'_>) -> CmdResult<Option<String>> {
    let s = state.inner().clone();
    blocking(move || {
        let Some(target) = app
            .dialog()
            .file()
            .add_filter("CSV", &["csv"])
            .set_file_name(format!(
                "runmarq-history-{}.csv",
                chrono::Local::now().format("%Y-%m-%d")
            ))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = target.into_path().map_err(|e| e.to_string())?;
        let csv = s.collector.call(|c| c.get_text("/v1/export.csv"))?;
        std::fs::write(&path, csv).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn delete_history(state: Shared<'_>, before: Option<String>) -> CmdResult<u64> {
    let s = state.inner().clone();
    blocking(move || {
        let q = match before {
            Some(b) => {
                let t = chrono::DateTime::parse_from_rfc3339(&b).map_err(|e| e.to_string())?;
                format!("?before={}", encode(&t.to_rfc3339()))
            }
            None => String::new(),
        };
        let v: serde_json::Value = s.collector.call(|c| c.delete(&format!("/v1/history{q}")))?;
        Ok(v["deleted"].as_u64().unwrap_or(0))
    })
    .await
}

#[tauri::command]
pub async fn restart_collector(state: Shared<'_>) -> CmdResult<crate::collector::CollectorInfo> {
    let s = state.inner().clone();
    blocking(move || s.collector.restart()).await
}

#[tauri::command]
pub async fn open_path(app: AppHandle, state: Shared<'_>, kind: String) -> CmdResult<()> {
    let paths = &state.collector.paths;
    let p = match kind.as_str() {
        "data" => paths.root.clone(),
        "logs" => paths.logs_dir(),
        _ => return Err("unknown location".into()),
    };
    app.opener()
        .open_path(p.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Whether new terminals find the `runmarq` command.
#[tauri::command]
pub async fn terminal_command(state: Shared<'_>) -> CmdResult<bool> {
    let exe = binary::installed_path(&state.collector.paths.root);
    blocking(move || Ok(shell_path::status(&exe))).await
}

/// Adds or removes the `runmarq` command for new terminals. Returns whether
/// it is now available without further steps.
#[tauri::command]
pub async fn set_terminal_command(state: Shared<'_>, enabled: bool) -> CmdResult<bool> {
    let s = state.inner().clone();
    blocking(move || {
        let exe = s.collector.prepare_cli()?;
        if enabled {
            shell_path::install(&exe).map(|i| i.on_path)
        } else {
            shell_path::remove(&exe).map(|_| false)
        }
    })
    .await
}

#[tauri::command]
pub async fn set_widget_visible(app: AppHandle, visible: bool) -> CmdResult<()> {
    crate::set_widget(&app, visible);
    crate::tray::refresh(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding() {
        let q = HistoryQuery {
            search: Some("a b&c=%".into()),
            limit: Some(5),
            ..Default::default()
        };
        assert_eq!(history_path(&q), "/v1/runs?search=a%20b%26c%3D%25&limit=5");
    }
}
