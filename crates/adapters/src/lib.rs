//! Agent adapters.
//!
//! Each adapter knows, for one vendor:
//! - how to detect the installed agent and its version,
//! - which capabilities its official hook interface exposes,
//! - how to install, verify and remove runmarq's hook entries without
//!   touching unrelated configuration,
//! - how to translate the vendor's raw hook payload into normalized events.
//!
//! Adding an agent means adding one module here and registering it in
//! [`all`]. The collector, storage and estimator do not change.

pub mod binary;
pub mod claude;
pub mod codex;
pub mod common;
pub mod config;
pub mod cursor;
pub mod detect;
pub mod grouped;
pub mod shell_path;
pub mod status;
pub mod transcript;

use chrono::{DateTime, Utc};
use runmarq_core::api::HookConfig;
use runmarq_core::capabilities::{AgentCapabilities, SupportLevel};
use runmarq_core::event::{AgentSource, EventEnvelope};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub use detect::Detection;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("{0}")]
    Config(#[from] config::ConfigError),
    #[error("{0} is not installed or could not be found")]
    NotInstalled(&'static str),
    #[error("{0}")]
    Unsupported(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    /// Applies to every project for this user.
    User,
    /// Applies to one project directory only.
    Project { root: PathBuf },
}

impl Scope {
    pub fn label(&self) -> String {
        match self {
            Scope::User => "user".into(),
            Scope::Project { root } => format!("project:{}", root.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub scope: Scope,
    /// Absolute path of the `runmarq` executable the hooks should invoke.
    pub executable: PathBuf,
    /// Where backups of modified configuration files are written.
    pub backups_dir: PathBuf,
    /// Overrides the user's home directory (tests).
    pub home: Option<PathBuf>,
}

impl InstallOptions {
    pub fn home(&self) -> PathBuf {
        self.home
            .clone()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// What setup will change, shown to the user before they approve it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub agent: AgentSource,
    pub config_path: PathBuf,
    pub file_exists: bool,
    pub already_installed: bool,
    /// Hook events that will be registered.
    pub events: Vec<String>,
    /// The exact command each hook will run.
    pub command: String,
    /// The JSON fragment runmarq adds, for display.
    pub preview: serde_json::Value,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub config_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub changed: bool,
    pub pending_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResult {
    pub config_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub removed_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub label: String,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub config_path: PathBuf,
    pub configured: bool,
    /// Every expected hook event is present and points at an existing executable.
    pub complete: bool,
    pub checks: Vec<Check>,
    pub pending_steps: Vec<String>,
}

/// Inputs to normalization that come from the environment, not the payload.
#[derive(Debug, Clone)]
pub struct NormalizeContext {
    /// When the hook process started.
    pub now: DateTime<Utc>,
    pub hook_config: HookConfig,
    /// Per-install salt for repository ids and prompt fingerprints.
    pub salt: Vec<u8>,
    /// Set by `runmarq run` in the child environment.
    pub launcher_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Normalized {
    pub events: Vec<EventEnvelope>,
    /// Exactly what the hook must print to stdout (some vendors require a
    /// JSON acknowledgement). `None` means print nothing.
    pub stdout: Option<&'static str>,
    /// Why the payload produced no events, for diagnostics.
    pub ignored: Option<String>,
}

pub trait Adapter: Send + Sync {
    fn source(&self) -> AgentSource;
    fn capabilities(&self) -> AgentCapabilities;
    fn support(&self) -> SupportLevel;
    /// Human-readable limitations, shown in the UI and docs.
    fn limitations(&self) -> Vec<String>;
    fn detect(&self, home: &Path) -> Detection;
    fn config_path(&self, scope: &Scope, home: &Path) -> PathBuf;
    fn plan(&self, opts: &InstallOptions) -> Result<InstallPlan>;
    fn install(&self, opts: &InstallOptions) -> Result<InstallResult>;
    fn verify(&self, opts: &InstallOptions) -> Verification;
    fn uninstall(&self, opts: &InstallOptions) -> Result<UninstallResult>;
    /// Translates one raw hook invocation. `declared_event` is the event name
    /// passed on the command line, used when the payload does not carry one.
    fn normalize(
        &self,
        ctx: &NormalizeContext,
        declared_event: Option<&str>,
        raw: &[u8],
    ) -> Normalized;
}

pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(claude::Claude),
        Box::new(codex::Codex),
        Box::new(cursor::Cursor),
    ]
}

pub fn for_source(source: AgentSource) -> Box<dyn Adapter> {
    match source {
        AgentSource::ClaudeCode => Box::new(claude::Claude),
        AgentSource::Codex => Box::new(codex::Codex),
        AgentSource::Cursor => Box::new(cursor::Cursor),
    }
}

/// Public capability manifest consumed by the website and desktop app.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub id: AgentSource,
    pub display_name: &'static str,
    pub support: SupportLevel,
    pub capabilities: AgentCapabilities,
    pub limitations: Vec<String>,
    pub mechanism: &'static str,
    pub docs_path: &'static str,
}

pub fn manifest() -> Vec<ManifestEntry> {
    all()
        .into_iter()
        .map(|a| ManifestEntry {
            id: a.source(),
            display_name: a.source().display_name(),
            support: a.support(),
            capabilities: a.capabilities(),
            limitations: a.limitations(),
            mechanism: match a.source() {
                AgentSource::ClaudeCode => "Claude Code hooks (settings.json)",
                AgentSource::Codex => {
                    "Codex lifecycle hooks (hooks.json) and the runmarq run wrapper"
                }
                AgentSource::Cursor => "Cursor agent hooks (hooks.json)",
            },
            docs_path: match a.source() {
                AgentSource::ClaudeCode => "/docs/integrations/claude-code",
                AgentSource::Codex => "/docs/integrations/codex",
                AgentSource::Cursor => "/docs/integrations/cursor",
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// The committed manifest used by the website must match the adapters.
    /// Regenerate with `runmarq integrations manifest > packages/contracts/integrations.json`.
    #[test]
    fn committed_manifest_is_current() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/contracts/integrations.json"
        );
        let generated = serde_json::to_value(super::manifest()).unwrap();
        let committed: serde_json::Value = match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap(),
            Err(_) => {
                std::fs::write(
                    path,
                    serde_json::to_string_pretty(&generated).unwrap() + "\n",
                )
                .unwrap();
                return;
            }
        };
        assert_eq!(
            committed, generated,
            "packages/contracts/integrations.json is stale; regenerate it"
        );
    }
}
