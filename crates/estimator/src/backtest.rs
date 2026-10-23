//! Replays finished runs in the order they ended. Each run is forecast at
//! several moments by an estimator fitted only on the runs that ended
//! before it, exactly as the live forecast would have been made, and the
//! shown range and outlook are checked against what actually happened.

use crate::training::{TrainingRun, TrainingSet};
use crate::{Estimator, ForecastInput};
use chrono::Duration;
use runmarq_core::forecast::{likely_range, ForecastStatus, Outlook};
use runmarq_core::run::RunState;
use serde::Serialize;

/// Runs that must have ended before any run is evaluated.
const MIN_TRAINING_RUNS: usize = 5;
/// Evaluated runs are split into this many blocks, each forecast by models
/// fitted on everything that ended before the block.
const BLOCKS: usize = 4;
const CHECKPOINTS: usize = 6;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub forecasts: usize,
    /// Actual remaining time inside the likely (P25–P75) range; 50% is ideal.
    pub likely_hits: usize,
    /// Inside P10–P90; 80% is ideal.
    pub range_hits: usize,
    /// The outlook shown matched what happened.
    pub outlook_hits: usize,
    /// Ended sooner than P10, or later than P90.
    pub sooner_than_range: usize,
    pub later_than_range: usize,
    log_errors: Vec<f64>,
    log_widths: Vec<f64>,
    /// Sum of the mean P10/P50/P90 pinball loss in ln seconds: a proper
    /// score that rewards both accuracy and honest ranges.
    loss_sum: f64,
}

/// Mean pinball loss of the P10, P50 and P90 forecasts, in ln seconds.
fn pinball(f: &Forecast) -> f64 {
    let y = f.actual.max(1.0).ln();
    [(0.1, f.range.0), (0.5, f.median), (0.9, f.range.1)]
        .iter()
        .map(|&(tau, q)| {
            let u = y - q.max(1.0).ln();
            u * (tau - f64::from(u < 0.0))
        })
        .sum::<f64>()
        / 3.0
}

impl Tally {
    fn add(&mut self, f: &Forecast) {
        self.forecasts += 1;
        self.loss_sum += pinball(f);
        self.likely_hits += usize::from(f.likely.0 <= f.actual && f.actual <= f.likely.1);
        self.range_hits += usize::from(f.range.0 <= f.actual && f.actual <= f.range.1);
        self.sooner_than_range += usize::from(f.actual < f.range.0);
        self.later_than_range += usize::from(f.actual > f.range.1);
        self.outlook_hits += usize::from(outlook_held(f.outlook, f.actual));
        self.log_errors
            .push((f.median.max(1.0) / f.actual.max(1.0)).ln().abs());
        self.log_widths
            .push((f.likely.1.max(1.0) / f.likely.0.max(1.0)).ln());
    }

    pub fn likely_rate(&self) -> Option<f64> {
        self.rate(self.likely_hits)
    }

    pub fn range_rate(&self) -> Option<f64> {
        self.rate(self.range_hits)
    }

    pub fn outlook_rate(&self) -> Option<f64> {
        self.rate(self.outlook_hits)
    }

    /// Typical factor between the median estimate and the actual time.
    pub fn error_factor(&self) -> Option<f64> {
        median(&self.log_errors).map(f64::exp)
    }

    /// Typical ratio of the likely range's upper end to its lower end.
    pub fn width_factor(&self) -> Option<f64> {
        median(&self.log_widths).map(f64::exp)
    }

    /// Mean quantile loss in ln seconds; lower is better.
    pub fn loss(&self) -> Option<f64> {
        (self.forecasts > 0).then(|| self.loss_sum / self.forecasts as f64)
    }

    fn rate(&self, hits: usize) -> Option<f64> {
        (self.forecasts > 0).then(|| hits as f64 / self.forecasts as f64)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub label: &'static str,
    #[serde(flatten)]
    pub tally: Tally,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub finished_runs: usize,
    pub evaluated_runs: usize,
    pub overall: Tally,
    pub by_moment: Vec<Moment>,
    /// Forecasts shown as "Finishing up", and how many ended within 3 min.
    pub finishing_forecasts: usize,
    pub finishing_within_3_min: usize,
}

struct Forecast {
    actual: f64,
    median: f64,
    likely: (f64, f64),
    range: (f64, f64),
    outlook: Outlook,
}

/// Whether the outlook shown was borne out by the remaining time. Each
/// band allows half again on its promise before it counts as wrong.
pub fn outlook_held(outlook: Outlook, remaining: f64) -> bool {
    match outlook {
        Outlook::FinishingUp | Outlook::AlmostDone => remaining <= 180.0,
        Outlook::FewMinutes => remaining <= 900.0,
        Outlook::AWhile => remaining > 180.0 && remaining <= 2700.0,
        Outlook::LongTask => remaining > 900.0,
    }
}

fn median(v: &[f64]) -> Option<f64> {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

fn moment_index(elapsed: f64) -> usize {
    match elapsed {
        t if t < 1.0 => 0,
        t if t < 120.0 => 1,
        t if t < 600.0 => 2,
        _ => 3,
    }
}

const MOMENTS: [&str; 4] = ["At the start", "First 2 min", "2 to 10 min", "After 10 min"];

pub fn run(set: &TrainingSet) -> Report {
    let mut runs: Vec<&TrainingRun> = set.runs.iter().collect();
    runs.sort_by_key(|r| r.ended_at);
    let labelled: Vec<usize> = (0..runs.len()).filter(|&i| runs[i].labelled()).collect();
    let mut report = Report {
        finished_runs: labelled.len(),
        by_moment: MOMENTS
            .iter()
            .map(|label| Moment {
                label,
                tally: Tally::default(),
            })
            .collect(),
        ..Default::default()
    };
    let first = labelled
        .iter()
        .position(|&i| i >= MIN_TRAINING_RUNS)
        .unwrap_or(labelled.len());
    let eval = &labelled[first..];
    if eval.is_empty() {
        return report;
    }
    let block = eval.len().div_ceil(BLOCKS);
    for chunk in eval.chunks(block) {
        let cut = chunk[0];
        let start_of_first =
            runs[cut].ended_at - Duration::seconds(runs[cut].active_seconds as i64);
        let train = TrainingSet {
            runs: runs[..cut]
                .iter()
                .filter(|r| r.ended_at <= start_of_first)
                .map(|r| (*r).clone())
                .collect(),
            commands: set.commands.clone(),
        };
        let est = Estimator::fit(&train, start_of_first);
        for &i in chunk {
            let r = runs[i];
            report.evaluated_runs += 1;
            for t in r.checkpoints(CHECKPOINTS) {
                let snapshot = r.snapshot_at(t);
                let actual = r.active_seconds - t;
                let now = r.ended_at - Duration::seconds(actual as i64);
                let f = est.forecast(&ForecastInput {
                    run_id: &r.id,
                    now,
                    agent: r.agent,
                    repository_id: r.repository_id.as_deref(),
                    prompt: r.prompt.as_ref(),
                    effort: r.effort.as_deref(),
                    model: r.model_at(t),
                    state: RunState::Running,
                    snapshot: &snapshot,
                    signals: r.signals_at(t),
                    repo_files: r.repo_files,
                    measured_coverage: None,
                });
                if f.status != ForecastStatus::Available {
                    continue;
                }
                let (Some(a), Some(m), Some(b), Some(outlook)) = (
                    f.remaining_p10_seconds,
                    f.remaining_p50_seconds,
                    f.remaining_p90_seconds,
                    f.outlook(),
                ) else {
                    continue;
                };
                let (lo, hi) = likely_range(a, m, b);
                let shown = Forecast {
                    actual,
                    median: f64::from(m),
                    likely: (f64::from(lo), f64::from(hi)),
                    range: (f64::from(a), f64::from(b)),
                    outlook,
                };
                if outlook == Outlook::FinishingUp {
                    report.finishing_forecasts += 1;
                    report.finishing_within_3_min += usize::from(actual <= 180.0);
                }
                report.overall.add(&shown);
                report.by_moment[moment_index(t)].tally.add(&shown);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Outcome;
    use chrono::Utc;
    use runmarq_core::event::{AgentSource, ComplexityBucket, PromptFeatures, TaskCategory};

    fn run(i: usize, secs: f64) -> TrainingRun {
        TrainingRun {
            id: format!("r{i}"),
            agent: AgentSource::ClaudeCode,
            repository_id: Some("repo".into()),
            prompt: Some(PromptFeatures {
                category: TaskCategory::BugFix,
                complexity: ComplexityBucket::Medium,
                complexity_score: 0.5,
                word_count: 10,
                list_items: 0,
                file_mentions: 0,
                mentions_tests: false,
                fingerprint: None,
                sketch: None,
            }),
            effort: None,
            model: None,
            model_at_start: None,
            outcome: Outcome::Completed,
            active_seconds: secs,
            ended_at: Utc::now() - Duration::hours(100 - i as i64),
            timeline: None,
            signals: Vec::new(),
            repo_files: None,
        }
    }

    #[test]
    fn nothing_to_evaluate_without_enough_earlier_runs() {
        let set = TrainingSet {
            runs: (0..4).map(|i| run(i, 300.0)).collect(),
            commands: Vec::new(),
        };
        let r = super::run(&set);
        assert_eq!(r.finished_runs, 4);
        assert_eq!(r.evaluated_runs, 0);
        assert_eq!(r.overall.likely_rate(), None);
    }

    #[test]
    fn steady_history_is_scored() {
        let set = TrainingSet {
            runs: (0..40)
                .map(|i| run(i, 300.0 * (1.0 + 0.15 * ((i % 5) as f64 - 2.0))))
                .collect(),
            commands: Vec::new(),
        };
        let r = super::run(&set);
        assert_eq!(r.evaluated_runs, 35);
        assert!(r.overall.forecasts >= 35);
        let range = r.overall.range_rate().unwrap();
        assert!(range > 0.6, "steady runs fall inside the range: {range}");
        assert!(r.overall.error_factor().unwrap() < 1.6);
        assert!(r.by_moment[0].tally.forecasts >= 35);
    }

    #[test]
    fn outlooks_allow_half_again_on_their_promise() {
        assert!(outlook_held(Outlook::AlmostDone, 150.0));
        assert!(!outlook_held(Outlook::AlmostDone, 400.0));
        assert!(outlook_held(Outlook::FewMinutes, 800.0));
        assert!(!outlook_held(Outlook::AWhile, 60.0));
        assert!(outlook_held(Outlook::LongTask, 3600.0));
    }
}
