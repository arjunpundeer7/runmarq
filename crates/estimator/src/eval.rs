//! Accuracy of the forecasts that were actually shown.
//!
//! Labels: a run's *remaining* active time at the moment each forecast was
//! made. Completed and failed runs both ended on the agent's side, so both are
//! valid labels for "time until the run ends". Censored runs (cancelled,
//! superseded, stale) are excluded from error metrics because their true
//! duration is unknown.
//!
//! No metric is reported until at least [`MIN_RUNS_FOR_METRICS`] runs can be
//! evaluated. A handful of runs cannot support a headline accuracy figure.

use crate::history::Outcome;
use runmarq_core::api::{AccuracyReport, ErrorPoint, HoldoutReport};
use runmarq_core::event::{AgentSource, TaskCategory};
use runmarq_core::forecast::{EtaForecast, ForecastStatus};
use std::collections::BTreeMap;

pub const MIN_RUNS_FOR_METRICS: u32 = 10;
/// Checkpoints (active seconds into a run) at which forecasts are scored.
pub const CHECKPOINTS: [(u32, &str); 4] = [
    (0, "At start"),
    (60, "After 1 min"),
    (180, "After 3 min"),
    (300, "After 5 min"),
];

#[derive(Debug, Clone)]
pub struct EvalRun {
    pub run_id: String,
    pub agent: AgentSource,
    pub category: TaskCategory,
    pub outcome: Outcome,
    pub active_seconds: f64,
    pub forecasts: Vec<EtaForecast>,
}

#[derive(Debug, Clone, Copy)]
struct Point {
    abs_error: f64,
    pct_error: f64,
    covered: bool,
}

fn score(f: &EtaForecast, total: f64) -> Option<Point> {
    if f.status != ForecastStatus::Available {
        return None;
    }
    let (p10, p50, p90) = (
        f.remaining_p10_seconds?,
        f.remaining_p50_seconds?,
        f.remaining_p90_seconds?,
    );
    let actual = (total - f.active_elapsed_seconds as f64).max(0.0);
    let abs_error = (p50 as f64 - actual).abs();
    Some(Point {
        abs_error,
        // Floor the denominator so very short remainders do not explode the percentage.
        pct_error: abs_error / actual.max(30.0),
        covered: (p10 as f64) <= actual && actual <= (p90 as f64),
    })
}

fn median(v: &mut [f64]) -> Option<f64> {
    crate::stats::median(v)
}

fn summarize(label: String, points: &[Point], runs: u32) -> ErrorPoint {
    let enough = runs >= MIN_RUNS_FOR_METRICS;
    let mut errs: Vec<f64> = points.iter().map(|p| p.abs_error).collect();
    ErrorPoint {
        label,
        runs,
        median_abs_error_seconds: if enough { median(&mut errs) } else { None },
        coverage: if enough && !points.is_empty() {
            Some(points.iter().filter(|p| p.covered).count() as f64 / points.len() as f64)
        } else {
            None
        },
    }
}

/// Picks the first forecast made at or after `checkpoint` active seconds.
fn at_checkpoint(forecasts: &[EtaForecast], checkpoint: u32) -> Option<&EtaForecast> {
    forecasts
        .iter()
        .find(|f| f.active_elapsed_seconds >= checkpoint && f.status == ForecastStatus::Available)
}

pub fn evaluate(runs: &[EvalRun], holdout: Option<HoldoutReport>) -> AccuracyReport {
    let labeled: Vec<&EvalRun> = runs
        .iter()
        .filter(|r| r.outcome != Outcome::Censored)
        .collect();

    let mut all_points = Vec::new();
    let mut by_age: Vec<(Vec<Point>, u32)> = CHECKPOINTS.iter().map(|_| (Vec::new(), 0)).collect();
    let mut by_agent: BTreeMap<&'static str, (Vec<Point>, u32)> = BTreeMap::new();
    let mut by_category: BTreeMap<&'static str, (Vec<Point>, u32)> = BTreeMap::new();
    let mut evaluated = 0u32;
    let mut stability = Vec::new();

    for run in &labeled {
        let mut any = false;
        for (i, (cp, _)) in CHECKPOINTS.iter().enumerate() {
            if run.active_seconds <= *cp as f64 {
                continue; // The run had already ended at this checkpoint.
            }
            let Some(p) =
                at_checkpoint(&run.forecasts, *cp).and_then(|f| score(f, run.active_seconds))
            else {
                continue;
            };
            any = true;
            all_points.push(p);
            by_age[i].0.push(p);
            by_age[i].1 += 1;
            if i == 0 {
                let a = by_agent.entry(run.agent.display_name()).or_default();
                a.0.push(p);
                a.1 += 1;
                let c = by_category.entry(run.category.label()).or_default();
                c.0.push(p);
                c.1 += 1;
            }
        }
        if any {
            evaluated += 1;
        }
        let avail: Vec<&EtaForecast> = run
            .forecasts
            .iter()
            .filter(|f| f.status == ForecastStatus::Available && f.remaining_p50_seconds.is_some())
            .collect();
        for w in avail.windows(2) {
            let dt = w[1]
                .active_elapsed_seconds
                .saturating_sub(w[0].active_elapsed_seconds) as f64;
            let prev = (w[0].remaining_p50_seconds.unwrap_or(0) as f64 - dt).max(1.0);
            let next = (w[1].remaining_p50_seconds.unwrap_or(0) as f64).max(1.0);
            stability.push((next / prev).ln().abs());
        }
    }

    let insufficient = runs
        .iter()
        .filter(|r| {
            r.forecasts.first().is_none_or(|f| {
                f.status == ForecastStatus::InsufficientData
                    || f.status == ForecastStatus::Unavailable
            })
        })
        .count();

    let enough = evaluated >= MIN_RUNS_FOR_METRICS;
    let mut errs: Vec<f64> = all_points.iter().map(|p| p.abs_error).collect();
    let mut pct: Vec<f64> = all_points.iter().map(|p| p.pct_error).collect();

    AccuracyReport {
        evaluated_runs: evaluated,
        minimum_runs_for_metrics: MIN_RUNS_FOR_METRICS,
        median_abs_error_seconds: if enough { median(&mut errs) } else { None },
        mean_abs_error_seconds: if enough && !errs.is_empty() {
            Some(errs.iter().sum::<f64>() / errs.len() as f64)
        } else {
            None
        },
        median_abs_pct_error: if enough { median(&mut pct) } else { None },
        interval_coverage: if enough && !all_points.is_empty() {
            Some(all_points.iter().filter(|p| p.covered).count() as f64 / all_points.len() as f64)
        } else {
            None
        },
        insufficient_info_fraction: if runs.len() as u32 >= MIN_RUNS_FOR_METRICS {
            Some(insufficient as f64 / runs.len() as f64)
        } else {
            None
        },
        stability: if enough && !stability.is_empty() {
            Some(stability.iter().sum::<f64>() / stability.len() as f64)
        } else {
            None
        },
        by_forecast_age: CHECKPOINTS
            .iter()
            .zip(by_age)
            .map(|((_, label), (pts, n))| summarize(label.to_string(), &pts, n))
            .collect(),
        by_agent: by_agent
            .into_iter()
            .map(|(k, (pts, n))| summarize(k.to_string(), &pts, n))
            .collect(),
        by_category: by_category
            .into_iter()
            .map(|(k, (pts, n))| summarize(k.to_string(), &pts, n))
            .collect(),
        holdout,
    }
}
