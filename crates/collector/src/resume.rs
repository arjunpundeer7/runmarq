//! Resuming Claude Code after its usage limit resets.
//!
//! When a run ends because Claude Code reached its plan's usage limit, and
//! the user has turned this on, the session is resumed in the background
//! once the limit resets: `claude --resume <session> -p ...` in the run's
//! folder, with the model and permission mode the session had. Its hooks
//! report the resumed work like any other run.
//!
//! A session that carries on before then, because the user sent a prompt or
//! Claude Code continued by itself, is left alone. Whether an open window
//! will continue by itself depends on where it runs and how it is set up,
//! so runmarq goes by what happens rather than guessing: if nothing has
//! continued a few minutes after the reset and the window is still open,
//! the work continues in a fork, so the session that window holds is never
//! written from two places.
//!
//! A scheduled resume needs the project folder's path, which runmarq never
//! stores, so it is held in memory only and dropped if the collector stops
//! before it is due.

use chrono::{DateTime, Duration, Utc};
use runmarq_adapters::detect::find_executable;
use runmarq_core::event::AgentProcess;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// What the resumed session is asked to do.
pub const PROMPT: &str =
    "The usage limit that stopped your last turn has reset. Continue the task where you left off.";
/// Extra wait after the limit resets, which gives an open Claude Code window
/// time to continue on its own first.
pub const MARGIN: Duration = Duration::minutes(5);
/// Limits that reset further away than this, such as weekly ones, are not
/// waited for.
pub const MAX_WAIT: Duration = Duration::hours(24);
/// Resumes in a row that may stop at the limit again before runmarq stops
/// trying for that session.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Resume {
    /// The run that stopped at the limit.
    pub run_id: String,
    /// Claude Code's own session id.
    pub session_id: String,
    pub cwd: PathBuf,
    /// Claude Code's spelling, such as `acceptEdits`.
    pub permission_mode: Option<&'static str>,
    /// The model the run used, so the resume does not switch to the default.
    pub model: Option<String>,
    pub due: DateTime<Utc>,
    /// The Claude Code process that ran it.
    pub agent_process: Option<AgentProcess>,
    /// That process was still open when the resume came due.
    pub fork: bool,
    /// Repository folder name, for notifications.
    pub place: Option<String>,
}

/// Claude Code's spelling of a permission mode recorded in lower case.
/// Unknown modes are left out, which means Claude Code's default.
pub fn cli_permission_mode(recorded: &str) -> Option<&'static str> {
    match recorded {
        "acceptedits" => Some("acceptEdits"),
        "auto" => Some("auto"),
        "bypasspermissions" => Some("bypassPermissions"),
        "default" => Some("default"),
        "dontask" => Some("dontAsk"),
        "plan" => Some("plan"),
        _ => None,
    }
}

/// The arguments that resume `r` in print mode.
pub fn arguments(r: &Resume) -> Vec<String> {
    let mut args = vec![
        "--resume".to_string(),
        r.session_id.clone(),
        "-p".to_string(),
        PROMPT.to_string(),
    ];
    if r.fork {
        args.push("--fork-session".into());
    }
    if let Some(m) = r.permission_mode {
        args.push("--permission-mode".into());
        args.push(m.into());
    }
    if let Some(m) = r.model.as_deref().filter(|m| is_model_name(m)) {
        args.push("--model".into());
        args.push(m.into());
    }
    args
}

/// A model name as Claude Code reports it, such as `claude-opus-5-5`.
fn is_model_name(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 96
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._[]".contains(c))
}

/// Starts the resume in the background. Its output is discarded: the work
/// is in the session itself, where `claude --resume` shows it.
pub fn start(r: &Resume) -> Result<(), String> {
    let claude = find_executable(&["claude"]).ok_or("Claude Code was not found on the PATH")?;
    if !r.cwd.is_dir() {
        return Err(format!("{} no longer exists", r.cwd.display()));
    }
    let mut cmd = Command::new(claude);
    cmd.args(arguments(r))
        .current_dir(&r.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start Claude Code: {e}"))?;
    let session = r.session_id.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            tracing::warn!(%session, %status, "resumed Claude Code session ended with an error")
        }
        Err(e) => tracing::warn!(%session, "could not wait for the resumed session: {e}"),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resume(mode: Option<&'static str>) -> Resume {
        Resume {
            run_id: "run_1".into(),
            session_id: "abc-123".into(),
            cwd: PathBuf::from("."),
            permission_mode: mode,
            model: Some("claude-haiku-4-5".into()),
            due: Utc::now(),
            agent_process: None,
            fork: false,
            place: None,
        }
    }

    #[test]
    fn resumes_the_same_session_with_its_permission_mode() {
        assert_eq!(
            arguments(&resume(Some("acceptEdits"))),
            [
                "--resume",
                "abc-123",
                "-p",
                PROMPT,
                "--permission-mode",
                "acceptEdits",
                "--model",
                "claude-haiku-4-5"
            ]
        );
        let mut forked = resume(None);
        forked.fork = true;
        forked.model = Some("odd model; name".into());
        assert_eq!(
            arguments(&forked),
            ["--resume", "abc-123", "-p", PROMPT, "--fork-session"]
        );
    }

    #[test]
    fn permission_modes_are_spelled_as_claude_code_expects() {
        assert_eq!(cli_permission_mode("acceptedits"), Some("acceptEdits"));
        assert_eq!(
            cli_permission_mode("bypasspermissions"),
            Some("bypassPermissions")
        );
        assert_eq!(cli_permission_mode("something-new"), None);
    }
}
