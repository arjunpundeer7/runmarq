//! runmarq local collector.
//!
//! Receives normalized events from hooks, maintains run state, produces
//! forecasts, persists telemetry and serves a local API. Runs headless
//! (`runmarq collector start`) or embedded in the desktop app; a file lock
//! guarantees only one instance per user.

pub mod client;
pub mod engine;
pub mod integrations;
pub mod learning;
pub mod lifecycle;
pub mod migrate;
pub mod probe;
pub mod process;
pub mod server;
pub mod spool;
pub mod stats;
pub mod stuck;
pub mod tracker;
pub mod usage;

pub use client::Client;
pub use engine::Engine;

use runmarq_core::paths::Paths;

/// Logs go to daily-rotated files in the data directory (seven kept), never
/// to stdout, because stdout belongs to the hook protocol.
pub fn init_file_logging(
    paths: &Paths,
    prefix: &str,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(prefix)
        .filename_suffix("log")
        .max_log_files(7)
        .build(paths.logs_dir())
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("RUNMARQ_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}
