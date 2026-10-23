//! ETA estimation for coding agent runs.
//!
//! Four models forecast the remaining active time of a run:
//!
//! - [`history`]: a log-normal survival baseline fitted hierarchically to
//!   local history and adjusted by live evidence ([`evidence`]);
//! - [`phase`]: a semi-Markov model of phase transitions, simulated forward
//!   from the run's current state;
//! - [`similar`]: durations of earlier runs with similar prompts;
//! - [`gbm`]: quantile gradient boosting over run features, once enough
//!   runs have finished.
//!
//! Their quantiles are combined with weights chosen by temporal validation
//! ([`training`]), and the P10–P90 interval is calibrated with split-conformal
//! prediction ([`conformal`]). Remaining time is always conditioned on the
//! time already spent, so an estimate grows when a run outlives similar runs
//! instead of counting down to zero.

pub mod backtest;
pub mod classify;
pub mod command;
pub mod conformal;
pub mod eval;
pub mod evidence;
pub mod features;
pub mod gbm;
pub mod history;
pub mod phase;
pub mod priors;
pub mod rng;
pub mod similar;
pub mod stats;
pub mod timeline;
pub mod training;

use chrono::{DateTime, Duration, Utc};
use conformal::Conformal;
use features::Query;
use history::Level;
use runmarq_core::api::{CalibrationStatus, HoldoutReport, ModelComponent, ModelStatus};
use runmarq_core::event::{AgentSource, PromptFeatures};
use runmarq_core::forecast::{Confidence, EtaForecast, ForecastBasis, ForecastStatus};
use runmarq_core::run::RunState;
use runmarq_core::signals::RunSignals;
use stats::{norm_isf, norm_sf};
use timeline::RunSnapshot;
use training::{mixture, FitReport, Models, TrainingSet, DEFAULT_WEIGHTS};

pub const MODEL_VERSION: &str = "ensemble-2";

/// Hard cap so a pathological input can never produce absurd numbers.
pub const MAX_REMAINING_SECONDS: f64 = 24.0 * 3600.0;
/// Fraction of similar runs already outlived beyond which we say so.
const BEYOND_TYPICAL_SURVIVAL: f64 = 0.25;
/// Relative P50 change below which consecutive forecasts are smoothed.
pub const SMOOTHING_THRESHOLD: f64 = 1.35;
const LIVE_PATHS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    History,
    Phase,
    Similar,
    Learned,
}

impl Component {
    pub const ALL: [Component; 4] = [
        Component::History,
        Component::Phase,
        Component::Similar,
        Component::Learned,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Component::History => "history",
            Component::Phase => "phase",
            Component::Similar => "similar",
            Component::Learned => "learned",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Component::History => "History baseline",
            Component::Phase => "Phase simulation",
            Component::Similar => "Similar prompts",
            Component::Learned => "Learned model",
        }
    }

    fn reason(self) -> Option<&'static str> {
        match self {
            Component::History => None,
            Component::Phase => Some("phase_model"),
            Component::Similar => Some("similar_tasks"),
            Component::Learned => Some("learned_model"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForecastInput<'a> {
    pub run_id: &'a str,
    pub now: DateTime<Utc>,
    pub agent: AgentSource,
    pub repository_id: Option<&'a str>,
    pub prompt: Option<&'a PromptFeatures>,
    pub effort: Option<&'a str>,
    pub model: Option<&'a str>,
    pub state: RunState,
    pub snapshot: &'a RunSnapshot,
    pub signals: Option<&'a RunSignals>,
    pub repo_files: Option<u32>,
    /// Measured P10–P90 coverage of the forecasts shown so far, when known.
    pub measured_coverage: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Estimator {
    models: Models,
    weights: [f64; 4],
    conformal: Option<Conformal>,
    report: FitReport,
}

impl Default for Estimator {
    fn default() -> Self {
        Estimator {
            models: Models::default(),
            weights: DEFAULT_WEIGHTS,
            conformal: None,
            report: FitReport::default(),
        }
    }
}

/// Quantile `q` of remaining time for `ln T ~ N(mu, sigma^2)` given `T > t`.
pub fn conditional_remaining(mu: f64, sigma: f64, t: f64, q: f64) -> f64 {
    let q = q.clamp(1e-6, 1.0 - 1e-6);
    if t <= 0.0 {
        return (mu + sigma * stats::norm_ppf(q)).exp();
    }
    let z_t = (t.ln() - mu) / sigma;
    if z_t > 8.0 {
        // Far beyond the bulk of the distribution: the asymptotic hazard
        // z_t / (sigma * t) gives an exponential tail that stays stable and
        // continuous with the exact branch to within a few percent.
        let hazard = z_t / (sigma * t);
        return -(1.0 - q).ln() / hazard;
    }
    let s_t = norm_sf(z_t);
    let z_q = norm_isf((1.0 - q) * s_t);
    ((mu + sigma * z_q).exp() - t).max(0.0)
}

fn seconds(v: f64) -> u32 {
    v.clamp(1.0, MAX_REMAINING_SECONDS).round() as u32
}

fn ordered(v: [f64; 3]) -> [u32; 3] {
    let a = seconds(v[0]);
    let b = seconds(v[1]).max(a);
    [a, b, seconds(v[2]).max(b)]
}

impl Estimator {
    pub fn fit(data: &TrainingSet, now: DateTime<Utc>) -> Estimator {
        let f = training::fit(data, now);
        Estimator {
            models: f.models,
            weights: f.weights,
            conformal: f.conformal,
            report: f.report,
        }
    }

    pub fn report(&self) -> &FitReport {
        &self.report
    }

    pub fn weights(&self) -> [f64; 4] {
        self.weights
    }

    pub fn is_calibrated(&self) -> bool {
        self.conformal.is_some()
    }

    pub fn holdout(&self) -> Option<HoldoutReport> {
        self.report.holdout.clone()
    }

    pub fn set_commands(&mut self, stats: impl IntoIterator<Item = command::CommandStat>) {
        self.models.commands = command::CommandTable::new(stats);
    }

    /// Records a finished command so in-flight estimates use it before the next refit.
    pub fn observe_command(
        &mut self,
        repository_id: Option<&str>,
        key: &str,
        seconds: f64,
        failed: bool,
    ) {
        self.models
            .commands
            .observe(repository_id, key, seconds, failed);
    }

    fn active(&self, c: Component) -> bool {
        let i = c as usize;
        self.weights[i] > 0.0
            && match c {
                Component::History => true,
                Component::Phase => self.models.phase.is_some(),
                Component::Similar => !self.models.similar.is_empty(),
                Component::Learned => self.models.learned.is_some(),
            }
    }

    pub fn forecast(&self, input: &ForecastInput<'_>) -> EtaForecast {
        let elapsed = input.snapshot.active_elapsed.max(0.0);
        let mut out = EtaForecast {
            run_id: input.run_id.to_string(),
            generated_at: input.now,
            remaining_p10_seconds: None,
            remaining_p50_seconds: None,
            remaining_p90_seconds: None,
            expected_finish_at: None,
            confidence: Confidence::Low,
            status: ForecastStatus::Available,
            reason_codes: Vec::new(),
            model_version: MODEL_VERSION.to_string(),
            basis: ForecastBasis::ColdStart,
            sample_size: 0.0,
            active_elapsed_seconds: elapsed.round().min(u32::MAX as f64) as u32,
            raw_p10_seconds: None,
            raw_p50_seconds: None,
            raw_p90_seconds: None,
            calibrated: false,
            components: Vec::new(),
        };

        let paused = match input.state {
            s if s.is_terminal() => Some((ForecastStatus::Unavailable, "run_ended")),
            RunState::WaitingForUser => Some((ForecastStatus::Waiting, "waiting_for_user")),
            RunState::WaitingForExternalResource => {
                Some((ForecastStatus::Waiting, "waiting_for_external_resource"))
            }
            RunState::Unknown => Some((ForecastStatus::Unavailable, "status_unknown")),
            _ => None,
        };
        if let Some((status, reason)) = paused {
            out.status = status;
            out.reason_codes.push(reason.into());
            return out;
        }

        let query = Query {
            seed: input.run_id,
            agent: input.agent,
            repository_id: input.repository_id,
            prompt: input.prompt,
            effort: input.effort,
            model: input.model,
            snapshot: input.snapshot,
            signals: input.signals,
            repo_files: input.repo_files,
        };
        let pred = self.models.predict(&query, input.now, LIVE_PATHS, None);
        let fit = &pred.baseline.fit;

        if input.prompt.is_none() {
            out.reason_codes.push("no_prompt_information".into());
            if fit.total_weight < 3.0 {
                out.status = ForecastStatus::InsufficientData;
                out.reason_codes.push("insufficient_history".into());
                return out;
            }
        }
        out.reason_codes.push(fit.level.reason_code().into());
        if elapsed > 0.0
            && norm_sf((elapsed.ln() - pred.baseline.mu) / fit.sigma) < BEYOND_TYPICAL_SURVIVAL
        {
            out.reason_codes.push("elapsed_beyond_typical".into());
        }
        out.reason_codes
            .extend(pred.baseline.reasons.iter().map(|r| r.to_string()));

        let Some((mix, used)) = mixture(&pred.q, &self.weights) else {
            out.status = ForecastStatus::Unavailable;
            out.reason_codes.push("no_model".into());
            return out;
        };
        for c in Component::ALL {
            if used[c as usize] {
                out.components.push(c.key().into());
                if let Some(r) = c.reason() {
                    out.reason_codes.push(r.into());
                }
            }
        }
        if pred.command_known && used[Component::Phase as usize] {
            out.reason_codes.push("command_history".into());
        }

        let raw = ordered(mix.map(f64::exp));
        let calibrated = match &self.conformal {
            Some(c) => {
                out.calibrated = true;
                out.reason_codes.push("calibrated".into());
                c.apply(elapsed, mix)
            }
            None => mix,
        };
        let mut q = ordered(calibrated.map(f64::exp)).map(f64::from);
        if let Some((target, w)) = evidence::plan_target(input.snapshot) {
            for i in 0..3 {
                q[i] = (q[i].max(1.0).ln() * (1.0 - w) + target[i].ln() * w).exp();
            }
            out.reason_codes.push("plan_progress".into());
        }
        if evidence::finishing(input.snapshot) && self.models.finishing_confirmed() {
            // What followed the same signs in earlier runs, not the models.
            q = self.models.finishing;
            out.reason_codes
                .push(runmarq_core::forecast::FINISHING_UP.into());
        }
        let q = ordered(q);

        out.remaining_p10_seconds = Some(q[0]);
        out.remaining_p50_seconds = Some(q[1]);
        out.remaining_p90_seconds = Some(q[2]);
        out.raw_p10_seconds = Some(raw[0]);
        out.raw_p50_seconds = Some(raw[1]);
        out.raw_p90_seconds = Some(raw[2]);
        out.expected_finish_at = Some(input.now + Duration::seconds(q[1] as i64));
        out.sample_size = fit.n_eff as f32;
        out.basis = if fit.total_weight < 3.0 {
            ForecastBasis::ColdStart
        } else if (fit.level >= Level::AgentCategory && fit.n_eff >= 12.0)
            || used[Component::Learned as usize]
        {
            ForecastBasis::History
        } else {
            ForecastBasis::Blended
        };
        out.confidence = confidence(
            out.basis,
            fit.n_eff,
            q[2] as f64 / q[0].max(1) as f64,
            input.prompt.is_some(),
            out.calibrated,
            input.measured_coverage,
        );
        out.reason_codes.dedup();
        out
    }

    pub fn status(&self) -> ModelStatus {
        let r = &self.report;
        let validation = |c: Component| {
            let v = &r.components[c as usize];
            match v.median_abs_log_error {
                Some(e) if v.validated_runs > 0 => format!(
                    " On {} runs it had not seen, its median was typically off by {:.1}x.",
                    v.validated_runs,
                    e.exp()
                ),
                _ => String::new(),
            }
        };
        let not_helping = "Trained, but on recent runs it did not improve the combined forecast.";
        let component = |c: Component, detail: String| ModelComponent {
            key: c.key().into(),
            label: c.label().into(),
            active: self.active(c),
            detail,
        };

        let history = if r.finished_runs == 0 {
            "Built-in starting points until runs finish.".to_string()
        } else {
            format!(
                "Fitted to {} finished runs by agent, task type, repository, effort and model.{}",
                r.finished_runs,
                validation(Component::History)
            )
        };
        let phase = match &self.models.phase {
            None => format!(
                "Needs {} finished runs with tool activity; has {}.",
                phase::MIN_RUNS,
                r.timeline_runs
            ),
            Some(_) if self.weights[Component::Phase as usize] == 0.0 => not_helping.into(),
            Some(m) => format!(
                "Simulates the rest of a run from the phase transitions of {} runs.{}",
                m.runs(),
                validation(Component::Phase)
            ),
        };
        let similar = if self.models.similar.is_empty() {
            "Needs finished runs with prompt sketches; none yet.".to_string()
        } else if self.weights[Component::Similar as usize] == 0.0 {
            not_helping.into()
        } else {
            format!(
                "Compares each prompt with {} earlier prompts using salted hashes, when similar ones exist.{}",
                self.models.similar.len(),
                validation(Component::Similar)
            )
        };
        let learned = match &self.models.learned {
            None if r.finished_runs < training::LEARNED_MIN_RUNS => format!(
                "Trains after {} finished runs; has {}.",
                training::LEARNED_MIN_RUNS,
                r.finished_runs
            ),
            None => "Did not beat the other models on recent runs, so it is not used.".into(),
            Some(_) => format!(
                "Gradient-boosted quantile model trained on {} moments from {} runs.{}",
                self.models.learned_rows,
                r.finished_runs,
                validation(Component::Learned)
            ),
        };
        let required_runs = training::VALIDATION_MIN_RUNS
            .max((conformal::MIN_RUNS as f64 / 0.6).ceil() as usize)
            as u32;
        ModelStatus {
            version: MODEL_VERSION.into(),
            trained_at: r.trained_at,
            finished_runs: r.finished_runs as u32,
            components: vec![
                component(Component::History, history),
                component(Component::Phase, phase),
                component(Component::Similar, similar),
                component(Component::Learned, learned),
            ],
            calibration: CalibrationStatus {
                active: self.conformal.is_some(),
                scores: self
                    .conformal
                    .as_ref()
                    .map(|c| c.scores)
                    .unwrap_or(r.validation_samples) as u32,
                validation_runs: r.validation_runs as u32,
                required_runs,
                target_coverage: conformal::TARGET_COVERAGE,
            },
        }
    }
}

/// Confidence describes the evidence behind an estimate. `High` requires a
/// calibrated interval, a well-populated cohort, a reasonably narrow range,
/// and, where measured, shown forecasts that covered about 80% of outcomes.
pub fn confidence(
    basis: ForecastBasis,
    n_eff: f64,
    width_ratio: f64,
    prompt_known: bool,
    calibrated: bool,
    coverage: Option<f64>,
) -> Confidence {
    if basis == ForecastBasis::ColdStart || !prompt_known || n_eff < 8.0 || width_ratio > 12.0 {
        return Confidence::Low;
    }
    let coverage_ok = coverage.is_none_or(|c| (0.7..=0.92).contains(&c));
    if calibrated && coverage_ok && n_eff >= 25.0 && width_ratio <= 5.0 {
        Confidence::High
    } else {
        Confidence::Medium
    }
}

/// Reason codes that justify an immediate, unsmoothed change.
const EVIDENCE_REASONS: &[&str] = &[
    "repeated_test_failures",
    "test_failure",
    "tool_failures",
    "tests_passing",
    "subagents_active",
    "plan_progress",
    "plan_complete",
];

/// Smooths `next` against the previous forecast of the same run.
///
/// Small changes are blended geometrically so the display does not jitter.
/// Large changes, or changes driven by new evidence, are passed through and
/// tagged `estimate_increased` / `estimate_decreased` so the UI can say why.
pub fn smooth(prev: Option<&EtaForecast>, mut next: EtaForecast) -> EtaForecast {
    let Some(prev) = prev else { return next };
    if prev.run_id != next.run_id
        || prev.status != ForecastStatus::Available
        || next.status != ForecastStatus::Available
    {
        return next;
    }
    let (Some(p10), Some(p50), Some(p90)) = (
        prev.remaining_p10_seconds,
        prev.remaining_p50_seconds,
        prev.remaining_p90_seconds,
    ) else {
        return next;
    };
    let (Some(n10), Some(n50), Some(n90)) = (
        next.remaining_p10_seconds,
        next.remaining_p50_seconds,
        next.remaining_p90_seconds,
    ) else {
        return next;
    };
    // Age the previous forecast by the active time that has passed since.
    let dt = next
        .active_elapsed_seconds
        .saturating_sub(prev.active_elapsed_seconds) as f64;
    let aged = [p10, p50, p90].map(|v| (v as f64 - dt).max(1.0));
    let new = [n10, n50, n90].map(|v| (v as f64).max(1.0));
    let ratio = new[1] / aged[1];
    let new_evidence = next
        .reason_codes
        .iter()
        .any(|r| EVIDENCE_REASONS.contains(&r.as_str()) && !prev.reason_codes.contains(r));

    if ratio.ln().abs() < SMOOTHING_THRESHOLD.ln() && !new_evidence {
        let blended =
            [0, 1, 2].map(|i| (0.5 * aged[i].ln() + 0.5 * new[i].ln()).exp().round() as u32);
        next.remaining_p10_seconds = Some(blended[0]);
        next.remaining_p50_seconds = Some(blended[1].max(blended[0]));
        next.remaining_p90_seconds = Some(blended[2].max(blended[1]));
        next.expected_finish_at = Some(next.generated_at + Duration::seconds(blended[1] as i64));
        next.reason_codes.push("smoothed".into());
    } else if ratio > 1.0 {
        next.reason_codes.push("estimate_increased".into());
    } else {
        next.reason_codes.push("estimate_decreased".into());
    }
    next
}

/// Evidence that pushes an estimate up, and how it is explained.
const RAISING: &[(&str, &str)] = &[
    ("repeated_test_failures", "after repeated test failures"),
    ("test_failure", "after a test failure"),
    ("tool_failures", "after several failed tool calls"),
    ("subagents_active", "while subagents are working"),
    ("plan_progress", "based on plan progress"),
    (
        "elapsed_beyond_typical",
        "because this run is taking longer than similar runs",
    ),
];
/// Evidence that pulls an estimate down.
const LOWERING: &[(&str, &str)] = &[
    ("plan_complete", "after all plan steps completed"),
    ("plan_progress", "based on plan progress"),
    ("tests_passing", "after tests passed"),
];

/// A short, factual explanation for a significant forecast change. Only a
/// cause that pushes the estimate the way it moved is named: a test failure
/// never explains a shorter estimate.
pub fn change_message(f: &EtaForecast) -> Option<String> {
    let increased = f.reason_codes.iter().any(|r| r == "estimate_increased");
    let decreased = f.reason_codes.iter().any(|r| r == "estimate_decreased");
    if !increased && !decreased {
        return None;
    }
    let has = |c: &str| f.reason_codes.iter().any(|r| r == c);
    let causes = if increased { RAISING } else { LOWERING };
    let cause = causes
        .iter()
        .find(|(code, _)| has(code))
        .map_or("as the run progressed", |(_, text)| *text);
    Some(format!(
        "Estimate {} {cause}.",
        if increased { "increased" } else { "decreased" }
    ))
}

#[cfg(test)]
mod tests;
