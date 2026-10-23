//! `runmarq collector start|stop|status`

use crate::ui::{self, mark};
use crate::{CollectorCmd, Output};
use anyhow::{bail, Context, Result};
use runmarq_collector::server::{run_until_shutdown, CollectorOptions};
use runmarq_collector::{lifecycle, Client};
use runmarq_core::paths::Paths;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

pub fn running_client(paths: &Paths) -> Option<Client> {
    let c = Client::discover(paths, Duration::from_secs(2)).ok()?;
    c.health().ok().map(|_| c)
}

/// Starts a detached background collector and waits until it answers.
pub fn spawn_background(paths: &Paths) -> Result<Client> {
    if let Some(c) = running_client(paths) {
        return Ok(c);
    }
    // Run the long-lived process from the stable copy, so the executable the
    // user invoked (a build output, an app bundle) is never held open.
    let current = std::env::current_exe()?;
    let exe = runmarq_adapters::binary::ensure_installed(&current, &paths.root).unwrap_or(current);
    let mut cmd = Command::new(exe);
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
    cmd.spawn().context("starting the collector")?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Some(c) = running_client(paths) {
            return Ok(c);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!(
        "the collector did not start within 8 seconds; see logs in {}",
        paths.logs_dir().display()
    )
}

pub fn run(paths: &Paths, cmd: CollectorCmd) -> Result<ExitCode> {
    match cmd {
        CollectorCmd::Start { foreground: true } => {
            lifecycle::ensure_dirs(paths)?;
            let _guard = runmarq_collector::init_file_logging(paths, "collector");
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;
            rt.block_on(run_until_shutdown(CollectorOptions {
                paths: paths.clone(),
            }))?;
            Ok(ExitCode::SUCCESS)
        }
        CollectorCmd::Start { foreground: false } => {
            if running_client(paths).is_some() {
                println!("{} The collector is already running.", mark(true));
                return Ok(ExitCode::SUCCESS);
            }
            let c = spawn_background(paths)?;
            println!(
                "{} Collector started (pid {}, 127.0.0.1:{}).",
                mark(true),
                c.discovery.pid,
                c.discovery.port
            );
            Ok(ExitCode::SUCCESS)
        }
        CollectorCmd::Stop => {
            let Some(c) = running_client(paths) else {
                println!("The collector is not running.");
                return Ok(ExitCode::SUCCESS);
            };
            c.shutdown()?;
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline && lifecycle::is_locked(paths) {
                std::thread::sleep(Duration::from_millis(100));
            }
            if lifecycle::is_locked(paths) {
                bail!("the collector did not stop within 8 seconds");
            }
            println!("{} Collector stopped.", mark(true));
            Ok(ExitCode::SUCCESS)
        }
        CollectorCmd::Status(Output { json }) => {
            let c = running_client(paths);
            if json {
                ui::print_json(&serde_json::json!({
                    "running": c.is_some(),
                    "pid": c.as_ref().map(|c| c.discovery.pid),
                    "port": c.as_ref().map(|c| c.discovery.port),
                    "version": c.as_ref().map(|c| c.discovery.version.clone()),
                }))?;
            } else {
                match &c {
                    Some(c) => println!(
                        "{} Running (pid {}, 127.0.0.1:{}, v{}).",
                        mark(true),
                        c.discovery.pid,
                        c.discovery.port,
                        c.discovery.version
                    ),
                    None => println!(
                        "{} Not running. Start it with `runmarq collector start`.",
                        mark(false)
                    ),
                }
            }
            Ok(if c.is_some() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(3)
            })
        }
    }
}
