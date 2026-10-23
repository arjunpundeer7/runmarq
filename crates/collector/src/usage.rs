//! Token usage per agent: today, the last seven days, and five-hour windows,
//! the period Claude plans meter usage in, applied to every agent.
//!
//! The counts are the ones agents wrote to their local transcripts and that
//! runmarq already reads for estimates (see `probe`). Nothing else is opened
//! and nothing leaves the machine. They are not the vendor's meter, so no
//! limit is ever assumed. Runs are the unit: a run's counts are spread evenly
//! over its duration, so a run that crosses a boundary is split by time.

use crate::engine::{Engine, Result};
use chrono::{DateTime, Duration, Local, Utc};
use runmarq_adapters::transcript::TranscriptFormat;
use runmarq_core::api::{AgentUsage, UsageReport, UsageTotals, UsageWindow};
use runmarq_core::event::AgentSource;
use runmarq_storage::RunRow;
use std::collections::HashMap;

pub const WINDOW_HOURS: u32 = 5;
/// Seven days, plus room for the window that was already open seven days ago.
const LOOKBACK: Duration = Duration::days(8);

fn window() -> Duration {
    Duration::hours(WINDOW_HOURS as i64)
}

/// One run's counts and the time it was active.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub agent: AgentSource,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub responses: u32,
    pub output_tokens: u64,
    pub thinking_tokens: u64,
    pub active_ms: u64,
}

impl Sample {
    pub fn of(row: &RunRow, now: DateTime<Utc>) -> Sample {
        let s = row.signals.clone().unwrap_or_default();
        let last = s
            .last_turn_at
            .map_or(row.last_event_at, |t| t.max(row.last_event_at));
        let end = row.ended_at.unwrap_or(last).min(now).max(row.started_at);
        let wall = (end - row.started_at).num_milliseconds().max(0) as u64;
        Sample {
            agent: row.agent,
            start: row.started_at,
            end,
            responses: s.turns.saturating_add(s.subagent_turns),
            output_tokens: s.output_tokens,
            thinking_tokens: s.thinking_tokens.min(s.output_tokens),
            active_ms: row
                .active_duration_ms
                .unwrap_or_else(|| wall.saturating_sub(row.waiting_duration_ms.unwrap_or(0))),
        }
    }

    /// The fraction of this run that falls in `[from, to)`.
    fn share(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
        if self.end <= self.start {
            return if from <= self.start && self.start < to {
                1.0
            } else {
                0.0
            };
        }
        let (a, b) = (self.start.max(from), self.end.min(to));
        if b <= a {
            return 0.0;
        }
        (b - a).num_milliseconds() as f64 / (self.end - self.start).num_milliseconds() as f64
    }
}

fn totals(samples: &[&Sample], from: DateTime<Utc>, to: DateTime<Utc>) -> UsageTotals {
    let (mut runs, mut responses, mut output, mut thinking, mut active) =
        (0u32, 0.0, 0.0, 0.0, 0.0);
    for s in samples {
        let k = s.share(from, to);
        if k <= 0.0 {
            continue;
        }
        runs += 1;
        responses += s.responses as f64 * k;
        output += s.output_tokens as f64 * k;
        thinking += s.thinking_tokens as f64 * k;
        active += s.active_ms as f64 * k;
    }
    UsageTotals {
        runs,
        responses: responses.round() as u32,
        output_tokens: output.round() as u64,
        thinking_tokens: thinking.round() as u64,
        active_ms: active.round() as u64,
    }
}

/// Start times of the five-hour windows these runs fall in, oldest first. A
/// window opens with the first activity after the previous one closed;
/// activity that carries on past its end opens the next one straight away.
pub fn window_starts(samples: &[&Sample]) -> Vec<DateTime<Utc>> {
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> =
        samples.iter().map(|s| (s.start, s.end)).collect();
    spans.sort();
    let mut starts: Vec<DateTime<Utc>> = Vec::new();
    for (start, end) in spans {
        if starts.last().is_none_or(|&last| start >= last + window()) {
            starts.push(start);
        }
        while let Some(&last) = starts.last() {
            if end <= last + window() {
                break;
            }
            starts.push(last + window());
        }
    }
    starts
}

/// The report for `samples` as of `now`. Only agents used in the last seven
/// days are listed.
pub fn aggregate(
    samples: &[Sample],
    now: DateTime<Utc>,
    today_started_at: DateTime<Utc>,
    transcripts_enabled: bool,
    budgets: &HashMap<AgentSource, u64>,
) -> UsageReport {
    let week = now - Duration::days(7);
    let until = now + Duration::seconds(1);
    let agents = AgentSource::ALL
        .into_iter()
        .filter_map(|agent| {
            let mine: Vec<&Sample> = samples.iter().filter(|s| s.agent == agent).collect();
            if !mine.iter().any(|s| s.end >= week) {
                return None;
            }
            let mut windows: Vec<UsageWindow> = window_starts(&mine)
                .into_iter()
                .map(|start| UsageWindow {
                    started_at: start,
                    ends_at: start + window(),
                    totals: totals(&mine, start, start + window()),
                })
                .collect();
            // Window starts are at least five hours apart, so at most the
            // last one is still open.
            let current_window = if windows.last().is_some_and(|w| w.ends_at > now) {
                windows.pop()
            } else {
                None
            };
            let recent_windows = windows
                .into_iter()
                .rev()
                .filter(|w| w.ends_at > week)
                .collect();
            Some(AgentUsage {
                agent,
                reports_tokens: TranscriptFormat::for_source(agent).is_some(),
                today: totals(&mine, today_started_at, until),
                last_seven_days: totals(&mine, week, until),
                current_window,
                recent_windows,
                window_budget_tokens: budgets.get(&agent).copied().filter(|b| *b > 0),
            })
        })
        .collect();
    UsageReport {
        generated_at: now,
        today_started_at,
        window_hours: WINDOW_HOURS,
        transcripts_enabled,
        agents,
    }
}

fn local_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    now.with_timezone(&Local)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|m| m.and_local_timezone(Local).earliest())
        .map(|m| m.with_timezone(&Utc))
        .unwrap_or(now - Duration::days(1))
}

/// Usage from stored runs, with open runs as they stand in memory.
pub fn report(engine: &Engine, now: DateTime<Utc>) -> Result<UsageReport> {
    let since = now - LOOKBACK;
    let mut open: HashMap<String, RunRow> = engine
        .open_rows()
        .into_iter()
        .map(|r| (r.id.clone(), r))
        .collect();
    let mut rows = engine.store().runs_active_since(since)?;
    for r in rows.iter_mut() {
        if let Some(live) = open.remove(&r.id) {
            *r = live;
        }
    }
    rows.extend(open.into_values().filter(|r| r.last_event_at >= since));
    let samples: Vec<Sample> = rows.iter().map(|r| Sample::of(r, now)).collect();
    let prefs = engine.preferences();
    Ok(aggregate(
        &samples,
        now,
        local_midnight(now),
        prefs.read_transcripts,
        &prefs.usage_window_budgets,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use runmarq_core::event::TaskCategory;
    use runmarq_core::run::RunState;
    use runmarq_core::signals::RunSignals;
    use runmarq_storage::{SessionRow, Store};

    fn t(h: f64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap()
            + Duration::milliseconds((h * 3_600_000.0) as i64)
    }

    fn sample(agent: AgentSource, from: f64, to: f64, tokens: u64) -> Sample {
        Sample {
            agent,
            start: t(from),
            end: t(to),
            responses: 10,
            output_tokens: tokens,
            thinking_tokens: tokens / 4,
            active_ms: ((to - from) * 3_600_000.0) as u64,
        }
    }

    fn claude(from: f64, to: f64, tokens: u64) -> Sample {
        sample(AgentSource::ClaudeCode, from, to, tokens)
    }

    #[test]
    fn windows_open_with_activity_and_roll_over_while_it_continues() {
        let runs = [
            claude(0.0, 0.5, 0),
            claude(1.0, 2.0, 0),
            claude(4.5, 4.9, 0),
            claude(6.0, 6.2, 0),
            claude(9.0, 12.0, 0),
            claude(30.0, 30.0, 0),
        ];
        let refs: Vec<&Sample> = runs.iter().collect();
        assert_eq!(
            window_starts(&refs),
            vec![t(0.0), t(6.0), t(11.0), t(30.0)],
            "a gap starts a new window; work past the end starts the next at once"
        );
        let refs: Vec<&Sample> = runs.iter().rev().collect();
        assert_eq!(window_starts(&refs).len(), 4, "input order does not matter");
    }

    #[test]
    fn runs_that_cross_a_boundary_are_split_by_time() {
        let s = claude(4.0, 6.0, 1000);
        assert_eq!(s.share(t(0.0), t(5.0)), 0.5);
        assert_eq!(s.share(t(5.0), t(10.0)), 0.5);
        assert_eq!(s.share(t(6.0), t(10.0)), 0.0);
        let instant = claude(3.0, 3.0, 10);
        assert_eq!(instant.share(t(3.0), t(4.0)), 1.0);
        assert_eq!(instant.share(t(2.0), t(3.0)), 0.0);
    }

    #[test]
    fn report_covers_today_the_week_and_the_open_window() {
        let day = |d: f64, h: f64| d * 24.0 + h;
        let now = t(day(7.0, 14.0));
        let today = t(day(7.0, 0.0));
        let samples = vec![
            // Eight days ago: outside every period.
            claude(-24.0, -23.0, 99_000),
            // Two days ago, two runs in one window.
            claude(day(5.0, 9.0), day(5.0, 10.0), 4_000),
            claude(day(5.0, 12.0), day(5.0, 13.0), 6_000),
            // Today: a window from 08:00, and a run that carries on past its
            // end at 13:00 into the window open now.
            claude(day(7.0, 8.0), day(7.0, 10.0), 2_000),
            claude(day(7.0, 12.0), day(7.0, 14.0), 1_000),
            sample(AgentSource::Cursor, day(7.0, 1.0), day(7.0, 2.0), 0),
        ];
        let r = aggregate(&samples, now, today, true, &HashMap::new());
        assert_eq!(r.window_hours, 5);
        assert_eq!(
            r.agents.iter().map(|a| a.agent).collect::<Vec<_>>(),
            vec![AgentSource::ClaudeCode, AgentSource::Cursor],
            "Codex was not used"
        );
        let c = &r.agents[0];
        assert!(c.reports_tokens);
        assert_eq!((c.today.runs, c.today.output_tokens), (2, 3_000));
        assert_eq!(c.last_seven_days.output_tokens, 13_000);
        assert_eq!(c.last_seven_days.thinking_tokens, 3_250);
        assert_eq!(c.last_seven_days.responses, 40);

        let w = c.current_window.as_ref().unwrap();
        assert_eq!(w.started_at, t(day(7.0, 13.0)));
        assert_eq!(w.ends_at, t(day(7.0, 18.0)));
        assert_eq!((w.totals.runs, w.totals.output_tokens), (1, 500));

        let starts: Vec<_> = c.recent_windows.iter().map(|w| w.started_at).collect();
        assert_eq!(
            starts,
            vec![t(day(7.0, 8.0)), t(day(5.0, 9.0))],
            "newest first, and the window from eight days ago is left out"
        );
        assert_eq!(c.recent_windows[0].totals.output_tokens, 2_500);
        assert_eq!(c.recent_windows[1].totals.output_tokens, 10_000);
        assert_eq!(c.recent_windows[1].totals.runs, 2);

        let cursor = &r.agents[1];
        assert!(!cursor.reports_tokens);
        assert_eq!(cursor.today.runs, 1);
        assert_eq!(cursor.today.output_tokens, 0);
        assert!(
            cursor.current_window.is_none(),
            "its window closed at 06:00"
        );
    }

    #[test]
    fn no_window_is_open_after_five_quiet_hours() {
        let samples = vec![claude(0.0, 1.0, 100)];
        let r = aggregate(&samples, t(6.5), t(0.0), true, &HashMap::new());
        let c = &r.agents[0];
        assert!(c.current_window.is_none());
        assert_eq!(c.recent_windows.len(), 1);
    }

    #[test]
    fn budgets_are_the_users_own_and_per_agent() {
        let samples = vec![
            claude(0.0, 1.0, 100),
            sample(AgentSource::Codex, 0.0, 1.0, 100),
        ];
        let budgets = HashMap::from([(AgentSource::ClaudeCode, 50_000), (AgentSource::Codex, 0)]);
        let r = aggregate(&samples, t(2.0), t(0.0), true, &budgets);
        assert_eq!(r.agents[0].window_budget_tokens, Some(50_000));
        assert_eq!(r.agents[1].window_budget_tokens, None, "zero means none");
        let r = aggregate(&samples, t(2.0), t(0.0), true, &HashMap::new());
        assert!(
            r.agents.iter().all(|a| a.window_budget_tokens.is_none()),
            "never invented"
        );
    }

    fn row(id: &str, start: DateTime<Utc>, end: Option<DateTime<Utc>>, tokens: u64) -> RunRow {
        RunRow {
            id: id.into(),
            session_id: "ses_u".into(),
            source_turn_id: None,
            agent: AgentSource::ClaudeCode,
            repository_id: None,
            task_category: TaskCategory::BugFix,
            prompt_features: None,
            title: None,
            model: None,
            started_at: start,
            ended_at: end,
            last_event_at: end.unwrap_or(start),
            wall_duration_ms: None,
            active_duration_ms: None,
            waiting_duration_ms: Some(0),
            status: if end.is_some() {
                RunState::Completed
            } else {
                RunState::Running
            },
            exit_reason: None,
            current_activity: None,
            current_tool: None,
            tool_calls: 0,
            tool_failures: 0,
            test_runs: 0,
            plan_total: None,
            plan_completed: None,
            initial_p10_ms: None,
            initial_p50_ms: None,
            initial_p90_ms: None,
            effort: None,
            permission_mode: None,
            signals: Some(RunSignals {
                turns: 3,
                output_tokens: tokens,
                ..Default::default()
            }),
        }
    }

    #[test]
    fn stored_runs_are_read_and_old_ones_skipped() {
        // Stored timestamps have millisecond precision.
        let now = runmarq_storage::from_ms(Utc::now().timestamp_millis());
        let store = Store::open_in_memory().unwrap();
        store
            .insert_session(&SessionRow {
                id: "ses_u".into(),
                agent: AgentSource::ClaudeCode,
                source_session_id: "u".into(),
                repository_id: None,
                started_at: now - Duration::days(30),
                last_seen_at: now,
                status: "active".into(),
            })
            .unwrap();
        for r in [
            row(
                "old",
                now - Duration::days(20),
                Some(now - Duration::days(20)),
                7_000,
            ),
            row(
                "recent",
                now - Duration::hours(2),
                Some(now - Duration::hours(1)),
                1_200,
            ),
        ] {
            store.upsert_run(&r).unwrap();
        }
        let mut engine = Engine::new(store).unwrap();
        let mut prefs = engine.preferences().clone();
        prefs
            .usage_window_budgets
            .insert(AgentSource::ClaudeCode, 200_000);
        engine.set_preferences(prefs).unwrap();
        let r = report(&engine, now).unwrap();
        assert!(r.transcripts_enabled);
        let c = &r.agents[0];
        assert_eq!(c.window_budget_tokens, Some(200_000));
        assert_eq!(c.last_seven_days.output_tokens, 1_200);
        assert_eq!(c.last_seven_days.runs, 1);
        let w = c.current_window.as_ref().unwrap();
        assert_eq!(w.started_at, now - Duration::hours(2));
        assert!(r.today_started_at <= now && now - r.today_started_at <= Duration::hours(25));
    }
}
