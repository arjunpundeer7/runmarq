//! `runmarq wait`: blocks until the run in progress finishes and exits with
//! its outcome, so a script can chain on an agent's work, as in
//! `runmarq wait && npm test`.

use crate::collector_cmd::running_client;
use crate::ui::{self, mark};
use crate::Output;
use anyhow::Result;
use clap::Args;
use runmarq_collector::client::ClientError;
use runmarq_collector::Client;
use runmarq_core::api::{RunDetail, RunView};
use runmarq_core::event::AgentSource;
use runmarq_core::paths::Paths;
use runmarq_core::run::{ExitReason, RunState};
use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// Polls that may fail in a row, for example while the collector restarts.
const MAX_MISSED_POLLS: u32 = 5;
const LINE_WIDTH: usize = 78;

#[derive(Args)]
#[command(after_long_help = "\
Exit codes:
  0    the run finished
  1    the run failed
  2    the run was cancelled, or its status is unknown
  3    no matching run is in progress
  124  --timeout passed first

If several runs match, waits for the one that started last.

Example:
  runmarq wait --agent claude && npm test")]
pub struct WaitArgs {
    /// Only wait for runs of this agent: claude, codex or cursor.
    #[arg(long, value_parser = parse_agent)]
    pub agent: Option<AgentSource>,
    /// Only wait for runs in the repository with this folder name.
    #[arg(long, value_name = "NAME")]
    pub repo: Option<String>,
    /// Stop waiting after this many seconds.
    #[arg(long, value_name = "SECS")]
    pub timeout: Option<u64>,
    /// No progress line, result or bell; only the exit code (and --json output).
    #[arg(long, short)]
    pub quiet: bool,
    #[command(flatten)]
    pub out: Output,
}

fn parse_agent(s: &str) -> Result<AgentSource, String> {
    AgentSource::parse(s).ok_or_else(|| format!("unknown agent `{s}`; use claude, codex or cursor"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Finished,
    Failed,
    Cancelled,
    Unknown,
    NothingRunning,
    TimedOut,
}

impl Outcome {
    pub fn code(self) -> u8 {
        match self {
            Outcome::Finished => 0,
            Outcome::Failed => 1,
            Outcome::Cancelled | Outcome::Unknown => 2,
            Outcome::NothingRunning => 3,
            Outcome::TimedOut => 124,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Finished => "finished",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
            Outcome::Unknown => "unknown",
            Outcome::NothingRunning => "nothing_running",
            Outcome::TimedOut => "timed_out",
        }
    }
}

/// Status unknown because the collector restarted clears with the agent's
/// next event; status unknown because the agent went silent does not.
fn restarted(r: &RunView) -> bool {
    r.exit_reason.as_deref() == Some(ExitReason::CollectorRestarted.as_str())
}

/// The outcome once the run has settled, `None` while it is in progress.
/// `tracked` is whether the collector still holds the run in memory.
pub fn settled(r: &RunView, tracked: bool) -> Option<Outcome> {
    match r.state {
        RunState::Completed => Some(Outcome::Finished),
        RunState::Failed => Some(Outcome::Failed),
        RunState::Cancelled => Some(Outcome::Cancelled),
        RunState::Unknown if tracked && restarted(r) => None,
        _ if !tracked => Some(Outcome::Unknown),
        RunState::Unknown => Some(Outcome::Unknown),
        _ => None,
    }
}

/// The run to wait for: the latest-started open run matching the filters.
pub fn pick<'a>(
    runs: &'a [RunView],
    agent: Option<AgentSource>,
    repo: Option<&str>,
) -> Option<&'a RunView> {
    runs.iter()
        .filter(|r| settled(r, true).is_none())
        .filter(|r| agent.is_none_or(|a| r.agent == a))
        .filter(|r| {
            repo.is_none_or(|want| {
                r.repository_name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(want.trim()))
            })
        })
        .max_by_key(|r| r.started_at)
}

fn describe(r: &RunView) -> String {
    let repo = r
        .repository_name
        .as_deref()
        .map(|n| format!(" · {n}"))
        .unwrap_or_default();
    format!("{}{repo}", r.agent.display_name())
}

pub fn progress_line(r: &RunView) -> String {
    // The estimate goes last: on a narrow line its tail is what gets cut.
    let line = format!(
        "Waiting for {} · {} elapsed · {}",
        describe(r),
        ui::duration(r.wall_ms),
        ui::eta(r)
    );
    if line.chars().count() <= LINE_WIDTH {
        return line;
    }
    let mut cut: String = line.chars().take(LINE_WIDTH - 1).collect();
    cut.push('…');
    cut
}

pub fn result_line(outcome: Outcome, r: Option<&RunView>) -> String {
    let Some(r) = r else {
        return "No matching run is in progress.".into();
    };
    let who = describe(r);
    let after = ui::duration(r.wall_ms);
    match outcome {
        Outcome::Finished => format!("{} {who} finished after {after}.", mark(true)),
        Outcome::Failed => format!("{} {who} run failed after {after}.", mark(false)),
        Outcome::Cancelled => format!("{who} run was cancelled after {after}."),
        Outcome::Unknown => format!("Status unknown: no recent updates from {who}."),
        Outcome::TimedOut => format!("{who} is still running after {after}; stopped waiting."),
        Outcome::NothingRunning => "No matching run is in progress.".into(),
    }
}

/// Whether the terminal draws OSC 9;4 progress on its tab and taskbar
/// button. Others may show some OSC 9 forms as notifications, so the
/// sequence is sent only where it is known to mean progress.
fn taskbar_supported() -> bool {
    std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("ConEmuPID").is_some()
        || matches!(
            std::env::var("TERM_PROGRAM").as_deref(),
            Ok("ghostty" | "WezTerm")
        )
}

/// OSC 9;4 state and percent for a run: the share of its likely total time
/// already spent, paused while it waits for you, and indeterminate without
/// an estimate.
pub fn taskbar(r: &RunView) -> (u8, u8) {
    let share = r
        .forecast
        .as_ref()
        .and_then(|f| f.remaining_p50_seconds)
        .map(|left| {
            let spent = r.active_ms as f64 / 1000.0;
            (100.0 * spent / (spent + f64::from(left)).max(1.0))
                .round()
                .clamp(1.0, 99.0) as u8
        });
    match (r.state, share) {
        (RunState::WaitingForUser, p) => (4, p.unwrap_or(50)),
        (_, Some(p)) => (1, p),
        (_, None) => (3, 0),
    }
}

/// One line rewritten in place on an interactive terminal; changed lines
/// printed one after another otherwise. Where the terminal supports it,
/// the run's progress also shows on its tab and taskbar button.
struct Progress {
    enabled: bool,
    tty: bool,
    taskbar: bool,
    last: String,
    last_taskbar: Option<(u8, u8)>,
}

impl Progress {
    fn show(&mut self, r: &RunView) {
        if !self.enabled {
            return;
        }
        if self.taskbar {
            let now = taskbar(r);
            if self.last_taskbar != Some(now) {
                let mut err = std::io::stderr().lock();
                let _ = write!(err, "\x1b]9;4;{};{}\x07", now.0, now.1);
                let _ = err.flush();
                self.last_taskbar = Some(now);
            }
        }
        let line = progress_line(r);
        if line == self.last {
            return;
        }
        let mut err = std::io::stderr().lock();
        if self.tty {
            let pad = self
                .last
                .chars()
                .count()
                .saturating_sub(line.chars().count());
            let _ = write!(err, "\r{line}{}", " ".repeat(pad));
        } else {
            let _ = writeln!(err, "{line}");
        }
        let _ = err.flush();
        self.last = line;
    }

    fn clear(&mut self) {
        let mut err = std::io::stderr().lock();
        if self.enabled && self.tty && !self.last.is_empty() {
            let _ = write!(err, "\r{}\r", " ".repeat(self.last.chars().count()));
        }
        if self.last_taskbar.take().is_some() {
            let _ = write!(err, "\x1b]9;4;0;0\x07");
        }
        let _ = err.flush();
        self.last.clear();
    }
}

/// The run as the collector reports it now, and whether it is still tracked.
fn poll(
    client: &mut Option<Client>,
    paths: &Paths,
    id: &str,
) -> Result<(RunView, bool), ClientError> {
    // After a restart the collector listens on a new port with a new token.
    if client.is_none() {
        *client = Some(Client::discover(paths, REQUEST_TIMEOUT)?);
    }
    let c = client.as_ref().expect("set above");
    let result = c.get::<Vec<RunView>>("/v1/runs/active").and_then(|runs| {
        match runs.into_iter().find(|r| r.id == id) {
            Some(r) => Ok((r, true)),
            None => c
                .get::<RunDetail>(&format!("/v1/runs/{id}"))
                .map(|d| (d.run, false)),
        }
    });
    if result.is_err() {
        *client = None;
    }
    result
}

fn done(args: &WaitArgs, outcome: Outcome, run: Option<&RunView>) -> Result<ExitCode> {
    finish(args, outcome, run, result_line(outcome, run))
}

fn finish(
    args: &WaitArgs,
    outcome: Outcome,
    run: Option<&RunView>,
    message: String,
) -> Result<ExitCode> {
    let ended = !matches!(outcome, Outcome::NothingRunning | Outcome::TimedOut);
    if !args.quiet && ended && std::io::stderr().is_terminal() {
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(b"\x07");
        let _ = err.flush();
    }
    if args.out.json {
        ui::print_json(&serde_json::json!({
            "outcome": outcome.as_str(),
            "exitCode": outcome.code(),
            "run": run,
        }))?;
    } else if !args.quiet {
        println!("{message}");
    }
    Ok(ExitCode::from(outcome.code()))
}

pub fn run(paths: &Paths, args: WaitArgs) -> Result<ExitCode> {
    let deadline = args
        .timeout
        .map(|s| Instant::now() + Duration::from_secs(s));
    let mut client = running_client(paths);
    let target = client
        .as_ref()
        .and_then(|c| c.get::<Vec<RunView>>("/v1/runs/active").ok())
        .and_then(|runs| pick(&runs, args.agent, args.repo.as_deref()).cloned());
    let Some(mut run) = target else {
        return done(&args, Outcome::NothingRunning, None);
    };
    let tty = std::io::stderr().is_terminal();
    let mut progress = Progress {
        enabled: !args.quiet,
        tty,
        taskbar: tty && taskbar_supported(),
        last: String::new(),
        last_taskbar: None,
    };
    let mut missed = 0;
    loop {
        progress.show(&run);
        let wait = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    progress.clear();
                    return done(&args, Outcome::TimedOut, Some(&run));
                }
                left.min(POLL)
            }
            None => POLL,
        };
        std::thread::sleep(wait);
        match poll(&mut client, paths, &run.id) {
            Ok((r, tracked)) => {
                missed = 0;
                run = r;
                if let Some(outcome) = settled(&run, tracked) {
                    progress.clear();
                    return done(&args, outcome, Some(&run));
                }
            }
            Err(_) => {
                missed += 1;
                if missed > MAX_MISSED_POLLS {
                    progress.clear();
                    let message =
                        "Lost contact with the runmarq collector; the outcome is unknown.".into();
                    return finish(&args, Outcome::Unknown, Some(&run), message);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use clap::Parser;
    use runmarq_core::event::TaskCategory;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: WaitArgs,
    }

    fn view(
        id: &str,
        agent: AgentSource,
        repo: Option<&str>,
        state: RunState,
        start: u32,
    ) -> RunView {
        RunView {
            id: id.into(),
            session_id: "ses".into(),
            agent,
            repository_id: None,
            repository_name: repo.map(String::from),
            title: None,
            task_category: TaskCategory::BugFix,
            prompt_known: true,
            model: None,
            state,
            exit_reason: None,
            started_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, start, 0).unwrap(),
            ended_at: None,
            last_event_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, start, 0).unwrap(),
            wall_ms: 372_000,
            active_ms: 372_000,
            waiting_ms: 0,
            current_activity: None,
            current_tool: None,
            attention: None,
            attention_detail: None,
            tool_calls: 0,
            tool_failures: 0,
            plan: None,
            forecast: None,
            initial_p10_seconds: None,
            initial_p50_seconds: None,
            initial_p90_seconds: None,
            effort: None,
            phase: None,
            signals: None,
        }
    }

    #[test]
    fn taskbar_shows_the_share_of_the_likely_total_spent() {
        let mut r = view("a", AgentSource::ClaudeCode, None, RunState::Running, 0);
        assert_eq!(taskbar(&r), (3, 0), "no estimate: indeterminate");
        r.forecast = Some(
            serde_json::from_value(serde_json::json!({
                "runId": "a", "generatedAt": "2026-10-01T09:00:00Z",
                "remainingP10Seconds": 60, "remainingP50Seconds": 124, "remainingP90Seconds": 600,
                "confidence": "medium", "status": "available", "reasonCodes": [],
                "modelVersion": "ensemble-2", "basis": "history", "sampleSize": 10.0,
                "activeElapsedSeconds": 372, "calibrated": false, "components": []
            }))
            .unwrap(),
        );
        assert_eq!(taskbar(&r), (1, 75), "372 s spent of about 496");
        r.state = RunState::WaitingForUser;
        assert_eq!(taskbar(&r).0, 4, "paused while it waits for you");
    }

    #[test]
    fn parses_its_options() {
        let c = Cli::try_parse_from([
            "wait",
            "--agent",
            "claude",
            "--repo",
            "acme-web",
            "--timeout",
            "90",
            "--json",
            "-q",
        ])
        .unwrap();
        assert_eq!(c.args.agent, Some(AgentSource::ClaudeCode));
        assert_eq!(c.args.repo.as_deref(), Some("acme-web"));
        assert_eq!(c.args.timeout, Some(90));
        assert!(c.args.out.json && c.args.quiet);
        assert!(Cli::try_parse_from(["wait", "--agent", "vim"]).is_err());
        assert!(Cli::try_parse_from(["wait", "--timeout", "soon"]).is_err());
        let c = Cli::try_parse_from(["wait"]).unwrap();
        assert!(c.args.agent.is_none() && c.args.timeout.is_none() && !c.args.quiet);
    }

    #[test]
    fn exit_codes() {
        let codes: Vec<u8> = [
            Outcome::Finished,
            Outcome::Failed,
            Outcome::Cancelled,
            Outcome::Unknown,
            Outcome::NothingRunning,
            Outcome::TimedOut,
        ]
        .iter()
        .map(|o| o.code())
        .collect();
        assert_eq!(codes, vec![0, 1, 2, 2, 3, 124]);
    }

    #[test]
    fn outcomes_follow_the_run_state() {
        let mut r = view("a", AgentSource::Codex, None, RunState::Running, 0);
        assert_eq!(settled(&r, true), None);
        r.state = RunState::WaitingForUser;
        assert_eq!(
            settled(&r, true),
            None,
            "waiting for approval is still in progress"
        );
        for (state, outcome) in [
            (RunState::Completed, Outcome::Finished),
            (RunState::Failed, Outcome::Failed),
            (RunState::Cancelled, Outcome::Cancelled),
        ] {
            r.state = state;
            assert_eq!(settled(&r, false), Some(outcome));
        }
        r.state = RunState::Unknown;
        r.exit_reason = Some("stale".into());
        assert_eq!(settled(&r, true), Some(Outcome::Unknown));
        r.exit_reason = Some("collector_restarted".into());
        assert_eq!(
            settled(&r, true),
            None,
            "clears with the agent's next event"
        );
        assert_eq!(settled(&r, false), Some(Outcome::Unknown));
        r.state = RunState::Running;
        assert_eq!(
            settled(&r, false),
            Some(Outcome::Unknown),
            "no longer tracked"
        );
    }

    #[test]
    fn picks_the_latest_matching_open_run() {
        let mut stale = view(
            "stale",
            AgentSource::ClaudeCode,
            Some("acme-web"),
            RunState::Unknown,
            30,
        );
        stale.exit_reason = Some("stale".into());
        let runs = vec![
            view(
                "old",
                AgentSource::ClaudeCode,
                Some("acme-web"),
                RunState::Running,
                0,
            ),
            view(
                "new",
                AgentSource::ClaudeCode,
                Some("billing"),
                RunState::WaitingForUser,
                10,
            ),
            view(
                "codex",
                AgentSource::Codex,
                Some("acme-web"),
                RunState::Running,
                5,
            ),
            stale,
        ];
        let id = |r: Option<&RunView>| r.map(|r| r.id.clone());
        assert_eq!(id(pick(&runs, None, None)).as_deref(), Some("new"));
        assert_eq!(
            id(pick(&runs, Some(AgentSource::ClaudeCode), Some("ACME-web"))).as_deref(),
            Some("old")
        );
        assert_eq!(
            id(pick(&runs, Some(AgentSource::Codex), None)).as_deref(),
            Some("codex")
        );
        assert_eq!(pick(&runs, Some(AgentSource::Cursor), None), None);
        assert_eq!(pick(&runs, None, Some("elsewhere")), None);
        assert_eq!(pick(&[], None, None), None);
    }

    #[test]
    fn lines_are_short_and_plain() {
        let r = view(
            "a",
            AgentSource::ClaudeCode,
            Some("acme-web"),
            RunState::Running,
            0,
        );
        let line = progress_line(&r);
        assert!(
            line.starts_with("Waiting for Claude Code · acme-web"),
            "{line}"
        );
        assert!(line.contains(" · 6:12 elapsed · "), "{line}");
        let mut long = r.clone();
        long.repository_name = Some("x".repeat(200));
        assert_eq!(progress_line(&long).chars().count(), LINE_WIDTH);
        assert!(result_line(Outcome::Finished, Some(&r))
            .ends_with("Claude Code · acme-web finished after 6:12."));
        assert_eq!(
            result_line(Outcome::NothingRunning, None),
            "No matching run is in progress."
        );
    }
}
