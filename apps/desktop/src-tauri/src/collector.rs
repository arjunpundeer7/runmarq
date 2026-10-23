//! Keeps a collector running while the app is open and holds the client.
//!
//! The collector runs as its own process (the stable `runmarq` copy in the
//! data directory), so a UI crash or restart never interrupts collection,
//! and hooks keep spooling events even while the app is closed.

use runmarq_collector::client::ClientError;
use runmarq_collector::{lifecycle, Client};
use runmarq_core::paths::Paths;
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorInfo {
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub struct Collector {
    pub paths: Paths,
    client: Mutex<Option<Client>>,
    last_error: Mutex<Option<String>>,
    /// Stable path of the CLI used for hooks and the collector process.
    pub cli: Mutex<Option<PathBuf>>,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "runmarq.exe"
    } else {
        "runmarq"
    }
}

/// Finds the CLI shipped with the app (Tauri places sidecars next to the
/// main executable) or, in development, the newest workspace build.
pub fn bundled_cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let beside = dir.join(exe_name());
    if beside.is_file() {
        return Some(beside);
    }
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.file_name().and_then(|n| n.to_str()) == Some("target") {
            let newest = ["release", "debug"]
                .iter()
                .map(|p| d.join(p).join(exe_name()))
                .filter(|p| p.is_file())
                .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
            if newest.is_some() {
                return newest;
            }
        }
        cur = d.parent();
    }
    None
}

impl Collector {
    pub fn new(paths: Paths) -> Collector {
        Collector {
            paths,
            client: Mutex::new(None),
            last_error: Mutex::new(None),
            cli: Mutex::new(None),
        }
    }

    /// Ensures the stable CLI copy exists and is current.
    pub fn prepare_cli(&self) -> Result<PathBuf, String> {
        let stable = runmarq_adapters::binary::installed_path(&self.paths.root);
        let path = match bundled_cli() {
            Some(src) => runmarq_adapters::binary::ensure_installed(&src, &self.paths.root)
                .map_err(|e| {
                    format!("could not install the runmarq command-line component: {e}")
                })?,
            None if stable.is_file() => stable,
            None => {
                return Err(
                    "the runmarq command-line component is missing from this installation".into(),
                )
            }
        };
        *self.cli.lock().unwrap() = Some(path.clone());
        Ok(path)
    }

    fn healthy(&self) -> Option<Client> {
        let c = Client::discover(&self.paths, Duration::from_secs(3)).ok()?;
        c.health().ok().map(|_| c)
    }

    fn spawn(&self) -> Result<(), String> {
        let cli = match self.cli.lock().unwrap().clone() {
            Some(p) => p,
            None => self.prepare_cli()?,
        };
        let mut cmd = Command::new(cli);
        cmd.args(["collector", "start", "--foreground"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.spawn()
            .map_err(|e| format!("could not start the collector: {e}"))?;
        Ok(())
    }

    /// Returns a client for a healthy collector, starting one if needed.
    pub fn ensure_running(&self) -> Result<Client, String> {
        if let Some(c) = self.healthy() {
            *self.client.lock().unwrap() = Some(c.clone());
            *self.last_error.lock().unwrap() = None;
            return Ok(c);
        }
        // A collector holding the lock but not answering is still starting up.
        if !lifecycle::is_locked(&self.paths) {
            self.spawn()?;
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Some(c) = self.healthy() {
                *self.client.lock().unwrap() = Some(c.clone());
                *self.last_error.lock().unwrap() = None;
                return Ok(c);
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let err = format!(
            "the collector did not respond; logs are in {}",
            self.paths.logs_dir().display()
        );
        *self.last_error.lock().unwrap() = Some(err.clone());
        Err(err)
    }

    /// Runs a request, reconnecting (and restarting the collector) once on failure.
    pub fn call<T>(&self, f: impl Fn(&Client) -> Result<T, ClientError>) -> Result<T, String> {
        let cached = self.client.lock().unwrap().clone();
        if let Some(c) = cached {
            match f(&c) {
                Ok(v) => return Ok(v),
                Err(ClientError::Status(code, body)) => return Err(api_error(code, &body)),
                Err(_) => {}
            }
        }
        let c = self.ensure_running()?;
        f(&c).map_err(|e| match e {
            ClientError::Status(code, body) => api_error(code, &body),
            other => other.to_string(),
        })
    }

    pub fn info(&self) -> CollectorInfo {
        match self.healthy() {
            Some(c) => CollectorInfo {
                running: true,
                pid: Some(c.discovery.pid),
                port: Some(c.discovery.port),
                version: Some(c.discovery.version.clone()),
                error: None,
            },
            None => CollectorInfo {
                running: false,
                pid: None,
                port: None,
                version: None,
                error: self.last_error.lock().unwrap().clone(),
            },
        }
    }

    pub fn stop(&self) {
        if let Some(c) = self.healthy() {
            let _ = c.shutdown();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && lifecycle::is_locked(&self.paths) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        *self.client.lock().unwrap() = None;
    }

    pub fn restart(&self) -> Result<CollectorInfo, String> {
        self.stop();
        self.ensure_running()?;
        Ok(self.info())
    }
}

fn api_error(code: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| format!("collector returned {code}"))
}
