//! `runmarq run -- <command>`: launch an agent under runmarq.
//!
//! The child inherits the terminal (stdin, stdout, stderr), so interactive
//! agents behave exactly as when launched directly. Ctrl+C reaches the child
//! through the terminal as usual; the wrapper only declines to die from it, so
//! it can report the child's exit. The child's exit code is returned
//! unchanged (128 + signal on Unix when killed by a signal).
//!
//! The child gets `RUNMARQ_LAUNCHER_ID` in its environment. Hooks forward it,
//! so when the process exits, any run it left open is closed as cancelled
//! (`process_exited`) instead of lingering. This matters most for Codex,
//! which has no session-end hook.
//!
//! For `codex exec --json`, the wrapper additionally reads Codex's structured
//! JSONL stream (passing every byte through to stdout unchanged) and turns it
//! into events. Hooks are suppressed in that mode to avoid double counting.

use crate::emit::{LAUNCHER_ENV, SUPPRESS_ENV};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use runmarq_adapters::codex::ExecStream;
use runmarq_adapters::NormalizeContext;
use runmarq_collector::{lifecycle, spool, Client};
use runmarq_core::api::{HookConfig, IngestRequest};
use runmarq_core::paths::Paths;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

#[cfg(windows)]
mod signals {
    use windows_sys::Win32::Foundation::BOOL;
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT,
    };

    unsafe extern "system" fn handler(kind: u32) -> BOOL {
        // Swallow Ctrl+C / Ctrl+Break in the wrapper; the child receives its own copy.
        BOOL::from(kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT)
    }

    pub fn ignore_interrupts() {
        // A handler (unlike the NULL "ignore" flag) is not inherited by children.
        // SAFETY: registering a valid extern "system" function pointer.
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(unix)]
mod signals {
    extern "C" fn noop(_: libc::c_int) {}

    pub fn ignore_interrupts() {
        // A handler (unlike SIG_IGN) resets to the default on exec, so the child
        // still receives SIGINT/SIGQUIT normally.
        // SAFETY: installing an async-signal-safe no-op handler.
        unsafe {
            libc::signal(libc::SIGINT, noop as *const () as libc::sighandler_t);
            libc::signal(libc::SIGQUIT, noop as *const () as libc::sighandler_t);
        }
    }
}

fn is_codex_exec_json(cmd: &[String]) -> bool {
    // Split on both separators so Windows-style paths are handled on every OS.
    let name = cmd[0].rsplit(['/', '\\']).next().unwrap_or("");
    let prog = name
        .rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_lowercase();
    prog == "codex"
        && cmd.iter().any(|a| a == "exec" || a == "e")
        && cmd
            .iter()
            .any(|a| a == "--json" || a == "--experimental-json")
}

fn send(paths: &Paths, client: &mut Option<Client>, req: IngestRequest) {
    if client.is_none() {
        *client = Client::discover(paths, Duration::from_secs(2)).ok();
    }
    let ok = client
        .as_ref()
        .map(|c| c.ingest(&req).is_ok())
        .unwrap_or(false);
    if !ok {
        *client = None;
        let _ = spool::write(paths, &req.events);
    }
}

pub fn run(paths: &Paths, command: Vec<String>) -> Result<ExitCode> {
    if command.is_empty() {
        bail!("nothing to run; usage: runmarq run -- <command> [args...]");
    }
    let launcher_id = format!("l_{:016x}", rand::random::<u64>());
    let exec_json = is_codex_exec_json(&command);

    let program = which_program(&command[0]);
    let mut cmd = Command::new(&program);
    cmd.args(&command[1..]).env(LAUNCHER_ENV, &launcher_id);
    if exec_json {
        cmd.env(SUPPRESS_ENV, "1").stdout(Stdio::piped());
    }
    signals::ignore_interrupts();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("could not start {}", command[0]))?;

    let mut reader_thread = None;
    if exec_json {
        let stdout = child.stdout.take().expect("piped");
        let p = paths.clone();
        let lid = launcher_id.clone();
        reader_thread = Some(std::thread::spawn(move || {
            let ctx = NormalizeContext {
                now: Utc::now(),
                hook_config: std::fs::read(p.hook_config())
                    .ok()
                    .and_then(|b| serde_json::from_slice::<HookConfig>(&b).ok())
                    .unwrap_or_default(),
                salt: lifecycle::install_salt(&p),
                launcher_id: Some(lid),
            };
            let cwd = std::env::current_dir()
                .ok()
                .map(|d| d.to_string_lossy().to_string());
            let mut stream = ExecStream::new(cwd.as_deref(), &ctx);
            let mut client = None;
            let mut out = std::io::stdout();
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        // Pass through first, unchanged.
                        let _ = out.write_all(line.as_bytes());
                        let _ = out.flush();
                        let mut c = ctx.clone();
                        c.now = Utc::now();
                        let events = stream.line(&c, &line);
                        if !events.is_empty() {
                            send(
                                &p,
                                &mut client,
                                IngestRequest {
                                    events,
                                    hook_latency_ms: None,
                                    replayed: false,
                                },
                            );
                        }
                    }
                }
            }
        }));
    }

    let status = child.wait().context("waiting for the child process")?;
    if let Some(t) = reader_thread {
        let _ = t.join();
    }
    if let Ok(c) = Client::discover(paths, Duration::from_secs(2)) {
        let _ = c.send_no_content(
            "POST",
            "/v1/launcher/exit",
            &serde_json::json!({ "launcherId": launcher_id }),
        );
    }
    Ok(exit_code(status))
}

/// Resolves `codex` to `codex.cmd` etc. on Windows, so shims launch correctly.
fn which_program(name: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(name);
    if p.components().count() > 1 {
        return p.to_path_buf();
    }
    runmarq_adapters::detect::find_executable(&[name]).unwrap_or_else(|| p.to_path_buf())
}

fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    if let Some(c) = status.code() {
        // Exit with the exact code: Windows codes can exceed 255
        // (0xC000013A after Ctrl+C) and `ExitCode` would truncate them.
        std::process::exit(c);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return ExitCode::from((128 + sig).min(255) as u8);
        }
    }
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    use super::is_codex_exec_json;

    #[test]
    fn detects_exec_json() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(is_codex_exec_json(&v(&[
            "codex", "exec", "--json", "fix it"
        ])));
        assert!(is_codex_exec_json(&v(&[
            "C:\\tools\\codex.cmd",
            "exec",
            "--json"
        ])));
        assert!(!is_codex_exec_json(&v(&["codex"])));
        assert!(!is_codex_exec_json(&v(&["claude", "exec", "--json"])));
    }
}
