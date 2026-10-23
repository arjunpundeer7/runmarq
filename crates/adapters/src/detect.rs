//! Installation and version detection with bounded process execution.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub detected: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    /// The agent's own configuration directory, if present.
    pub config_dir: Option<PathBuf>,
    pub notes: Vec<String>,
}

pub fn find_executable(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|n| which::which(n).ok())
}

/// Runs `exe args...` with a timeout and returns trimmed stdout.
/// Never passes agent-provided strings: callers supply fixed arguments.
pub fn run_with_timeout(exe: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Extracts the first `X.Y.Z`-looking token from a version string.
pub fn parse_version(s: &str) -> Option<String> {
    s.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .map(|t| t.trim_start_matches('v'))
        .find(|t| {
            let parts: Vec<&str> = t.split('.').collect();
            parts.len() >= 2
                && parts
                    .iter()
                    .take(3)
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|t| t.chars().take(32).collect())
}

/// Compares dotted numeric versions. Non-numeric parts compare as 0.
pub fn version_at_least(version: &str, minimum: &str) -> bool {
    let parse = |v: &str| {
        v.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (parse(version), parse(minimum));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(
            parse_version("2.1.143 (Claude Code)").as_deref(),
            Some("2.1.143")
        );
        assert_eq!(
            parse_version("codex-cli 0.130.0").as_deref(),
            Some("0.130.0")
        );
        assert_eq!(
            parse_version("3.23.12\n2d29876d\nx64").as_deref(),
            Some("3.23.12")
        );
        assert_eq!(parse_version("nothing here"), None);
        assert!(version_at_least("0.130.0", "0.130.0"));
        assert!(version_at_least("2.1.143", "2.0.0"));
        assert!(!version_at_least("0.99.9", "0.130.0"));
    }
}
