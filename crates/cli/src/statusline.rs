//! `runmarq statusline`: one short line for a shell prompt, tmux, or Claude
//! Code's status line. It runs on every prompt, so it reads the discovery
//! file, makes one local API call and never migrates data. When the
//! collector cannot be reached it prints nothing and exits 0.

use crate::Output;
use clap::Args;
use runmarq_collector::Client;
use runmarq_core::api::{AttentionKind, RunView};
use runmarq_core::event::AgentSource;
use runmarq_core::forecast::{humanize_eta, ForecastStatus};
use runmarq_core::paths::Paths;
use serde::Serialize;
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

/// Upper bound for the whole call; a local round trip takes a few milliseconds.
const TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Args)]
#[command(after_long_help = "\
Placeholders for --format:
  {working}  runs in progress
  {waiting}  runs waiting for you
  {stuck}    runs that look stuck
  {next}     estimate for the run likely to finish first, such as \"about 3–7 min\"
  {agent}    that run's agent
  {repo}     that run's repository folder

Claude Code: add this to ~/.claude/settings.json, or to a project's
.claude/settings.json, using the full path to runmarq if it is not on PATH:
  \"statusLine\": { \"type\": \"command\", \"command\": \"runmarq statusline\" }

tmux:
  set -g status-right '#(runmarq statusline)'
  set -g status-interval 5

Prints nothing and exits 0 when the collector is not running.")]
pub struct StatuslineArgs {
    /// Print this template instead of the default line, for example
    /// "{working} running, next {next}".
    #[arg(long, value_name = "TEMPLATE")]
    pub format: Option<String>,
    #[command(flatten)]
    pub out: Output,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Next {
    pub run_id: String,
    pub agent: AgentSource,
    pub repository: Option<String>,
    /// The likely remaining time, for example "about 3–7 min".
    pub text: String,
    pub remaining_p10_seconds: u32,
    pub remaining_p50_seconds: u32,
    pub remaining_p90_seconds: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub working: u32,
    pub waiting: u32,
    pub stuck: u32,
    /// The working run expected to finish first, when one has an estimate.
    pub next: Option<Next>,
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

pub fn summarize(runs: &[RunView]) -> Summary {
    let mut s = Summary::default();
    for r in runs {
        if r.state.is_working() {
            s.working += 1;
        } else if r.state.is_waiting() {
            s.waiting += 1;
        }
        if r.attention == Some(AttentionKind::Stuck) {
            s.stuck += 1;
        }
    }
    s.next = runs
        .iter()
        .filter(|r| r.state.is_working())
        .filter_map(|r| {
            let f = r.forecast.as_ref()?;
            if f.status != ForecastStatus::Available {
                return None;
            }
            let (p10, p50, p90) = (
                f.remaining_p10_seconds?,
                f.remaining_p50_seconds?,
                f.remaining_p90_seconds?,
            );
            Some(Next {
                run_id: r.id.clone(),
                agent: r.agent,
                repository: r.repository_name.clone(),
                text: lower_first(&humanize_eta(p10, p50, p90).0),
                remaining_p10_seconds: p10,
                remaining_p50_seconds: p50,
                remaining_p90_seconds: p90,
            })
        })
        .min_by_key(|n| (n.remaining_p50_seconds, n.remaining_p90_seconds));
    s
}

/// The default line, such as `runmarq ● 2 working · next: about 3–7 min`.
pub fn line(s: &Summary) -> String {
    if s.working == 0 && s.waiting == 0 {
        return "runmarq ○ idle".into();
    }
    let mut parts = Vec::new();
    if s.working > 0 {
        parts.push(format!("{} working", s.working));
    }
    if s.waiting > 0 {
        parts.push(format!("{} waiting for you", s.waiting));
    }
    if s.stuck > 0 {
        parts.push(format!("{} may be stuck", s.stuck));
    }
    match &s.next {
        Some(n) => parts.push(format!("next: {}", n.text)),
        None if s.working > 0 => parts.push("estimating".into()),
        None => {}
    }
    format!("runmarq ● {}", parts.join(" · "))
}

pub fn render(template: &str, s: &Summary) -> String {
    let next = match &s.next {
        Some(n) => n.text.clone(),
        None if s.working > 0 => "estimating".into(),
        None => String::new(),
    };
    let agent = s
        .next
        .as_ref()
        .map(|n| n.agent.display_name())
        .unwrap_or("");
    let repo = s
        .next
        .as_ref()
        .and_then(|n| n.repository.as_deref())
        .unwrap_or("");
    template
        .replace("{working}", &s.working.to_string())
        .replace("{waiting}", &s.waiting.to_string())
        .replace("{stuck}", &s.stuck.to_string())
        .replace("{next}", &next)
        .replace("{agent}", agent)
        .replace("{repo}", repo)
}

fn active_runs(paths: &Paths) -> Option<Vec<RunView>> {
    Client::discover(paths, TIMEOUT)
        .ok()?
        .get("/v1/runs/active")
        .ok()
}

pub fn run(paths: &Paths, args: &StatuslineArgs) -> ExitCode {
    let Some(runs) = active_runs(paths) else {
        return ExitCode::SUCCESS;
    };
    let s = summarize(&runs);
    let text = if args.out.json {
        serde_json::to_string(&s).unwrap_or_default()
    } else {
        match &args.format {
            Some(t) => render(t, &s),
            None => line(&s),
        }
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use clap::Parser;
    use runmarq_core::event::TaskCategory;
    use runmarq_core::forecast::{Confidence, EtaForecast, ForecastBasis};
    use runmarq_core::run::RunState;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: StatuslineArgs,
    }

    fn view(id: &str, state: RunState, eta: Option<(u32, u32, u32)>) -> RunView {
        let at = Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap();
        RunView {
            id: id.into(),
            session_id: "ses".into(),
            agent: AgentSource::ClaudeCode,
            repository_id: None,
            repository_name: Some("acme-web".into()),
            title: None,
            task_category: TaskCategory::BugFix,
            prompt_known: true,
            model: None,
            state,
            exit_reason: None,
            started_at: at,
            ended_at: None,
            last_event_at: at,
            wall_ms: 60_000,
            active_ms: 60_000,
            waiting_ms: 0,
            current_activity: None,
            current_tool: None,
            attention: None,
            attention_detail: None,
            tool_calls: 0,
            tool_failures: 0,
            plan: None,
            forecast: eta.map(|(p10, p50, p90)| EtaForecast {
                run_id: id.into(),
                generated_at: at,
                remaining_p10_seconds: Some(p10),
                remaining_p50_seconds: Some(p50),
                remaining_p90_seconds: Some(p90),
                expected_finish_at: None,
                confidence: Confidence::Medium,
                status: ForecastStatus::Available,
                reason_codes: vec![],
                model_version: "test".into(),
                basis: ForecastBasis::History,
                sample_size: 10.0,
                active_elapsed_seconds: 60,
                raw_p10_seconds: None,
                raw_p50_seconds: None,
                raw_p90_seconds: None,
                calibrated: false,
                components: vec![],
            }),
            initial_p10_seconds: None,
            initial_p50_seconds: None,
            initial_p90_seconds: None,
            effort: None,
            phase: None,
            signals: None,
        }
    }

    #[test]
    fn the_default_line() {
        assert_eq!(line(&summarize(&[])), "runmarq ○ idle");
        let runs = [
            view("slow", RunState::Running, Some((1200, 1800, 3600))),
            view("fast", RunState::Running, Some((180, 300, 420))),
        ];
        let s = summarize(&runs);
        assert_eq!(s.next.as_ref().unwrap().run_id, "fast");
        let expected = format!(
            "runmarq ● 2 working · next: {}",
            lower_first(&humanize_eta(180, 300, 420).0)
        );
        assert_eq!(line(&s), expected);
        assert!(line(&s).starts_with("runmarq ● 2 working · next: about "));

        let mut stuck = view("stuck", RunState::Running, None);
        stuck.attention = Some(AttentionKind::Stuck);
        let runs = [stuck, view("w", RunState::WaitingForUser, None)];
        assert_eq!(
            line(&summarize(&runs)),
            "runmarq ● 1 working · 1 waiting for you · 1 may be stuck · estimating"
        );
        let runs = [view("w", RunState::WaitingForUser, None)];
        assert_eq!(line(&summarize(&runs)), "runmarq ● 1 waiting for you");
    }

    #[test]
    fn templates_fill_placeholders() {
        let runs = [view("a", RunState::Running, Some((180, 300, 420)))];
        let s = summarize(&runs);
        let next = s.next.as_ref().unwrap().text.clone();
        assert_eq!(
            render(
                "{working}/{waiting}/{stuck} {agent} in {repo}: {next} {unknown}",
                &s
            ),
            format!("1/0/0 Claude Code in acme-web: {next} {{unknown}}")
        );
        assert_eq!(render("[{next}]", &Summary::default()), "[]");
    }

    #[test]
    fn json_is_one_line() {
        let runs = [view("a", RunState::Running, Some((180, 300, 420)))];
        let json = serde_json::to_string(&summarize(&runs)).unwrap();
        assert!(!json.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["working"], 1);
        assert_eq!(v["next"]["remainingP50Seconds"], 300);
        assert_eq!(v["next"]["agent"], "claude_code");
    }

    #[test]
    fn prints_nothing_without_a_collector() {
        let dir = std::env::temp_dir().join(format!("runmarq-statusline-{}", std::process::id()));
        assert_eq!(active_runs(&Paths::at(&dir)), None);
    }

    #[test]
    fn parses_its_options() {
        let c = Cli::try_parse_from(["statusline", "--format", "{next}", "--json"]).unwrap();
        assert_eq!(c.args.format.as_deref(), Some("{next}"));
        assert!(c.args.out.json);
    }
}
