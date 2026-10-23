//! Terminal formatting helpers. Colors are used only on an interactive
//! terminal and never when NO_COLOR is set.

use std::io::IsTerminal;

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn paint(code: &str, s: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}

pub fn mark(ok: bool) -> String {
    if ok {
        green("✓")
    } else {
        red("✗")
    }
}

pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

pub fn print_json<T: serde::Serialize>(v: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Asks a yes/no question on the terminal. Defaults to "no" when stdin is not interactive.
pub fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{question} [y/N] ");
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// One-line ETA summary for a run view, matching the desktop wording.
pub fn eta(run: &runmarq_core::api::RunView) -> String {
    use runmarq_core::forecast::{eta_line, ForecastStatus};
    use runmarq_core::run::RunState;
    match run.state {
        RunState::WaitingForUser => return "Waiting for you".into(),
        RunState::WaitingForExternalResource => return "Waiting on external resource".into(),
        RunState::Unknown => return "Status unknown".into(),
        s if s.is_terminal() => return s.label().into(),
        _ => {}
    }
    match &run.forecast {
        Some(f) if f.status == ForecastStatus::Available => match f.words() {
            Some(w) => match &w.tail {
                Some(tail) => format!("{}, {tail}", eta_line(&w)),
                None => eta_line(&w),
            },
            None => "Estimating".into(),
        },
        Some(f) if f.status == ForecastStatus::InsufficientData => {
            "Not enough information yet".into()
        }
        _ => "Estimating".into(),
    }
}
