//! Fitting the models from finished runs, temporal validation, ensemble
//! weights and conformal calibration.
//!
//! Validation is rolling-origin: runs are ordered by end time, models are fitted
//! on everything before a cut and asked to forecast the runs after it, at
//! several moments of each run, as if live. Those out-of-sample forecasts
//! decide how much each model is trusted and give the conformal scores.

use crate::command::{CommandStat, CommandTable};
use crate::conformal::{Conformal, Score};
use crate::features::{self, Query};
use crate::gbm::{Dataset, Gbm, Params};
use crate::history::{CohortFit, CohortKey, HistoricalRun, History, Outcome, SURVIVOR_KAPPA};
use crate::phase::{PhaseModel, PhaseRun};
use crate::priors::log_baseline;
use crate::rng::Rng;
use crate::similar::{Candidates, Neighbours, SimilarIndex, SimilarRun};
use crate::timeline::{RunSnapshot, Segment, Timeline};
use crate::{conditional_remaining, evidence, Component, MAX_REMAINING_SECONDS};
use chrono::{DateTime, Utc};
use runmarq_core::api::HoldoutReport;
use runmarq_core::event::{AgentSource, PromptFeatures, TaskCategory};
use runmarq_core::signals::{effort_rank, RunSignals};

/// Finished runs needed before the learned model is trained for live use.
pub const LEARNED_MIN_RUNS: usize = 100;
const LEARNED_MIN_FIT_RUNS: usize = 70;
const LEARNED_MIN_VALIDATED_RUNS: usize = 10;
/// Finished runs needed before temporal validation starts.
pub const VALIDATION_MIN_RUNS: usize = 30;
const FOLD_CUTS: [f64; 4] = [0.4, 0.6, 0.8, 1.0];
const FOLD_MAX_RUNS: usize = 200;
const VALIDATION_CHECKPOINTS: usize = 6;
const TRAIN_CHECKPOINTS: usize = 12;
const MAX_TRAIN_ROWS: usize = 8000;
const VALIDATION_PATHS: usize = 150;
const MIN_STACK_RUNS: usize = 15;
const FIXED_CHECKPOINTS: [f64; 10] = [
    15.0, 30.0, 60.0, 120.0, 240.0, 480.0, 900.0, 1800.0, 3600.0, 7200.0,
];
const RECENCY_HALF_LIFE_DAYS: f64 = 60.0;
const RECENCY_FLOOR: f64 = 0.25;

/// Weights used before validation has measured anything. The phase
/// simulation leads because it starts from where the run actually is; the
/// history baseline only knows what kind of run it is.
pub const DEFAULT_WEIGHTS: [f64; 4] = [0.5, 1.0, 0.8, 0.0];
/// Learned weights are pulled toward the defaults with this many runs'
/// worth of weight: weights fitted on a few dozen runs are mostly noise.
const WEIGHT_PRIOR_RUNS: f64 = 50.0;
const WEIGHT_SAMPLES: usize = 1200;

#[derive(Debug, Clone)]
pub struct TrainingRun {
    pub id: String,
    pub agent: AgentSource,
    pub repository_id: Option<String>,
    pub prompt: Option<PromptFeatures>,
    pub effort: Option<String>,
    /// The model the run used, as finally recorded.
    pub model: Option<String>,
    /// The model known when the run began: the session's, from its
    /// earlier runs. The run's own model is known once its transcript is read.
    pub model_at_start: Option<String>,
    pub outcome: Outcome,
    pub active_seconds: f64,
    pub ended_at: DateTime<Utc>,
    pub timeline: Option<Timeline>,
    /// Signal snapshots by active seconds, in order.
    pub signals: Vec<(f64, RunSignals)>,
    pub repo_files: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct TrainingSet {
    pub runs: Vec<TrainingRun>,
    pub commands: Vec<CommandStat>,
}

impl TrainingRun {
    pub fn labelled(&self) -> bool {
        self.outcome != Outcome::Censored
    }

    pub fn historical(&self) -> HistoricalRun {
        HistoricalRun {
            agent: self.agent,
            repository_id: self.repository_id.clone(),
            category: self
                .prompt
                .as_ref()
                .map(|p| p.category)
                .unwrap_or(TaskCategory::Unknown),
            complexity: self.prompt.as_ref().map(|p| p.complexity),
            active_seconds: self.active_seconds,
            outcome: self.outcome,
            ended_at: self.ended_at,
            effort: effort_rank(self.effort.as_deref()),
            model: self.model.clone(),
        }
    }

    fn weight(&self, now: DateTime<Utc>) -> f64 {
        let age = (now - self.ended_at).num_seconds() as f64 / 86_400.0;
        let recency = 0.5f64
            .powf(age.max(0.0) / RECENCY_HALF_LIFE_DAYS)
            .max(RECENCY_FLOOR);
        let outcome = match self.outcome {
            Outcome::Completed => 1.0,
            Outcome::Failed | Outcome::Censored => 0.5,
        };
        recency * outcome
    }

    fn segments(&self) -> Option<Vec<Segment>> {
        let tl = self.timeline.as_ref().filter(|t| !t.is_empty())?;
        Some(tl.segments(tl.at_active(self.active_seconds)))
    }

    pub fn snapshot_at(&self, active: f64) -> RunSnapshot {
        match &self.timeline {
            Some(tl) => {
                let mut s = tl.snapshot(tl.at_active(active));
                s.active_elapsed = active;
                s
            }
            None => RunSnapshot {
                active_elapsed: active,
                segment_elapsed: active,
                ..Default::default()
            },
        }
    }

    pub fn signals_at(&self, active: f64) -> Option<&RunSignals> {
        self.signals
            .iter()
            .rev()
            .find(|(t, _)| *t <= active)
            .map(|(_, s)| s)
    }

    /// The model as a live forecast at `active` seconds would have known it:
    /// the session's at first, the run's own once its transcript was read.
    /// Using the final model from the start would leak the outcome, because
    /// runs too short to have their transcript read never get one.
    pub fn model_at(&self, active: f64) -> Option<&str> {
        match self.signals_at(active) {
            Some(_) => self.model.as_deref().or(self.model_at_start.as_deref()),
            None => self.model_at_start.as_deref(),
        }
    }

    pub fn query<'a>(&'a self, snapshot: &'a RunSnapshot) -> Query<'a> {
        Query {
            seed: &self.id,
            agent: self.agent,
            repository_id: self.repository_id.as_deref(),
            prompt: self.prompt.as_ref(),
            effort: self.effort.as_deref(),
            model: self.model_at(snapshot.active_elapsed),
            snapshot,
            signals: self.signals_at(snapshot.active_elapsed),
            repo_files: self.repo_files,
        }
    }

    /// Moments (active seconds) at which this run is forecast in training:
    /// the start, fixed ages, and right after some tool calls ended.
    pub fn checkpoints(&self, max: usize) -> Vec<f64> {
        let d = self.active_seconds;
        let mut c: Vec<f64> = FIXED_CHECKPOINTS
            .iter()
            .copied()
            .filter(|t| *t < d - 1.0)
            .collect();
        if let Some(segs) = self.segments() {
            let ends: Vec<f64> = segs
                .iter()
                .skip(1)
                .map(|s| s.start + s.tool_secs + 0.5)
                .filter(|t| *t > 1.0 && *t < d - 1.0)
                .collect();
            let take = 6.min(ends.len());
            for i in 0..take {
                c.push(ends[i * ends.len() / take]);
            }
        }
        c.sort_by(f64::total_cmp);
        c.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        let mut out = vec![0.0];
        let room = max.saturating_sub(1);
        if c.len() <= room {
            out.extend(c);
        } else if room > 0 {
            out.extend((0..room).map(|i| c[i * c.len() / room]));
        }
        out
    }
}

/// History baseline for one query, with its evidence reasons.
#[derive(Debug, Clone)]
pub struct Baseline {
    pub fit: CohortFit,
    pub mu: f64,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Prediction {
    /// Remaining time quantiles in ln(seconds), per [`Component`].
    pub q: [Option<[f64; 3]>; 4],
    pub baseline: Baseline,
    pub neighbours: Option<Neighbours>,
    pub command_known: bool,
}

fn to_log(v: [f64; 3]) -> [f64; 3] {
    let mut out = v.map(|x| x.clamp(1.0, MAX_REMAINING_SECONDS).ln());
    out.sort_by(f64::total_cmp);
    out
}

#[derive(Debug, Clone)]
pub struct Models {
    pub history: History,
    pub phase: Option<PhaseModel>,
    pub similar: SimilarIndex,
    pub learned: Option<Gbm>,
    pub commands: CommandTable,
    pub learned_rows: usize,
    /// Remaining seconds [p10, p50, p90] once a run shows signs of finishing.
    pub finishing: [f64; 3],
    /// Runs the finishing quantiles were measured on (0 means the default).
    pub finishing_runs: usize,
}

/// Remaining time after the finishing signs appear, before any are measured.
pub const FINISHING_DEFAULT: [f64; 3] = [15.0, 60.0, 240.0];
const FINISHING_MIN_RUNS: usize = 4;

/// What happened next in earlier runs at the first moment they showed signs
/// of finishing: remaining seconds [p10, p50, p90] and the number of runs.
fn finishing_quantiles(runs: &[&TrainingRun], now: DateTime<Utc>) -> ([f64; 3], usize) {
    let mut seen: Vec<(f64, f64)> = Vec::new();
    for r in runs.iter().filter(|r| r.labelled()) {
        let Some(segs) = r.segments() else { continue };
        for s in &segs {
            let t = s.start + s.tool_secs + evidence::FINISHING_QUIET_SECONDS;
            if s.gap_secs() < evidence::FINISHING_QUIET_SECONDS || t >= r.active_seconds {
                continue;
            }
            if evidence::finishing(&r.snapshot_at(t)) {
                seen.push(((r.active_seconds - t).max(1.0).ln(), r.weight(now)));
                break;
            }
        }
    }
    if seen.len() < FINISHING_MIN_RUNS {
        return (FINISHING_DEFAULT, 0);
    }
    let sw: f64 = seen.iter().map(|x| x.1).sum();
    let mean = seen.iter().map(|x| x.0 * x.1).sum::<f64>() / sw;
    let var = seen.iter().map(|x| x.1 * (x.0 - mean).powi(2)).sum::<f64>() / sw;
    let sd = var.sqrt().max(0.35) * (1.0 + 1.0 / seen.len() as f64).sqrt();
    let z = 1.281_551_6;
    (
        [(mean - z * sd).exp(), mean.exp(), (mean + z * sd).exp()],
        seen.len(),
    )
}

impl Default for Models {
    fn default() -> Self {
        Models {
            history: History::default(),
            phase: None,
            similar: SimilarIndex::default(),
            learned: None,
            commands: CommandTable::default(),
            learned_rows: 0,
            finishing: FINISHING_DEFAULT,
            finishing_runs: 0,
        }
    }
}

/// Earlier runs must have ended within this long, typically, after showing
/// the finishing signs before a live run is said to be finishing up.
const FINISHING_MAX_MEDIAN: f64 = 120.0;

impl Models {
    /// Whether this user's own history shows that runs with the finishing
    /// signs really did end soon after. Until it does, the signs are not
    /// claimed: in some workflows "tests pass" is a midpoint, not the end.
    pub fn finishing_confirmed(&self) -> bool {
        self.finishing_runs > 0 && self.finishing[1] <= FINISHING_MAX_MEDIAN
    }

    pub fn fit(
        runs: &[&TrainingRun],
        commands: CommandTable,
        now: DateTime<Utc>,
        learned: bool,
    ) -> Models {
        let history = History::new(runs.iter().map(|r| r.historical()));
        let segments: Vec<(&TrainingRun, Vec<Segment>)> = runs
            .iter()
            .filter_map(|r| Some((*r, r.segments()?)))
            .collect();
        let phase = PhaseModel::fit(segments.iter().map(|(r, s)| {
            PhaseRun {
                agent: r.agent,
                category: r
                    .prompt
                    .as_ref()
                    .map(|p| p.category)
                    .unwrap_or(TaskCategory::Unknown),
                segments: s,
                ended: r.labelled(),
                weight: r.weight(now),
            }
        }));
        let similar = SimilarIndex::new(runs.iter().filter(|r| r.labelled()).filter_map(|r| {
            Some(SimilarRun {
                sketch: r.prompt.as_ref()?.sketch.clone()?,
                agent: r.agent,
                active_seconds: r.active_seconds,
                weight: r.weight(now),
            })
        }));
        let (finishing, finishing_runs) = finishing_quantiles(runs, now);
        let mut models = Models {
            history,
            phase,
            similar,
            learned: None,
            commands,
            learned_rows: 0,
            finishing,
            finishing_runs,
        };
        if learned {
            let data = models.learned_dataset(runs, now);
            models.learned_rows = data.rows();
            let params = Params {
                min_leaf_weight: (data.rows() as f64 * 0.01).max(8.0),
                ..Params::default()
            };
            models.learned = Gbm::fit(&data, &params, 17);
        }
        models
    }

    fn learned_dataset(&self, runs: &[&TrainingRun], now: DateTime<Utc>) -> Dataset {
        let labelled: Vec<&&TrainingRun> = runs.iter().filter(|r| r.labelled()).collect();
        let per_run = (MAX_TRAIN_ROWS / labelled.len().max(1)).clamp(2, TRAIN_CHECKPOINTS);
        let mut data = Dataset::new(features::COUNT);
        for r in labelled {
            let cps = r.checkpoints(per_run);
            let w = r.weight(now) / (cps.len() as f64).sqrt();
            for t in cps {
                let snap = r.snapshot_at(t);
                let x = features::encode(&r.query(&snap), &self.commands);
                data.push(&x, (r.active_seconds - t).max(1.0).ln(), w);
            }
        }
        let mean = data.w.iter().sum::<f64>() / data.w.len().max(1) as f64;
        if mean > 0.0 {
            data.w.iter_mut().for_each(|w| *w /= mean);
        }
        data
    }

    fn cohort<'a>(q: &Query<'a>) -> CohortKey<'a> {
        CohortKey {
            agent: q.agent,
            repository_id: q.repository_id,
            category: q.category(),
            effort: effort_rank(q.effort),
            model: q.model,
        }
    }

    pub fn baseline(&self, q: &Query<'_>, now: DateTime<Utc>) -> Baseline {
        let category = q.category();
        let fit = self.history.fit(&Self::cohort(q), now);
        let mu = log_baseline(category, q.prompt.map(|p| p.complexity)) + fit.offset;
        Baseline {
            fit,
            mu,
            reasons: Vec::new(),
        }
    }

    pub fn predict(
        &self,
        q: &Query<'_>,
        now: DateTime<Utc>,
        paths: usize,
        candidates: Option<&Candidates>,
    ) -> Prediction {
        let elapsed = q.elapsed();
        let mut baseline = self.baseline(q, now);
        let fitted = [0.1, 0.5, 0.9]
            .map(|p| conditional_remaining(baseline.mu, baseline.fit.sigma, elapsed, p));
        let log_target = log_baseline(q.category(), q.prompt.map(|p| p.complexity))
            + baseline.fit.effort_offset
            + baseline.fit.model_offset;
        let base = match self
            .history
            .survivors(&Self::cohort(q), now, elapsed, log_target)
        {
            Some((seen, n)) => {
                let w = n / (n + SURVIVOR_KAPPA);
                let mut out = [0.0; 3];
                for i in 0..3 {
                    out[i] = (w * seen[i] + (1.0 - w) * fitted[i].max(1.0).ln()).exp();
                }
                out
            }
            None => fitted,
        };
        let (adjusted, reasons) = evidence::adjust(q.snapshot, base);
        baseline.reasons = reasons;

        let phase = self.phase.as_ref().map(|m| {
            let mut rng = Rng::seeded(q.seed, elapsed as u64);
            to_log(m.remaining(
                q.agent,
                q.category(),
                q.snapshot,
                &self.commands,
                q.repository_id,
                &mut rng,
                paths,
            ))
        });
        let neighbours = match candidates {
            Some(c) => c.remaining(elapsed),
            None if !self.similar.is_empty() => q
                .prompt
                .and_then(|p| p.sketch.as_deref())
                .and_then(|s| self.similar.query(s, q.agent, elapsed)),
            None => None,
        };
        let learned = self.learned.as_ref().map(|g| {
            let mut v = g.predict(&features::encode(q, &self.commands));
            v.iter_mut()
                .for_each(|x| *x = x.clamp(0.0, MAX_REMAINING_SECONDS.ln()));
            v
        });
        let command_known = q
            .snapshot
            .in_flight
            .as_ref()
            .and_then(|f| f.command_key.as_deref())
            .is_some_and(|k| self.commands.lookup(q.repository_id, k).is_some());
        Prediction {
            q: [
                Some(to_log(adjusted)),
                phase,
                neighbours.map(|n| to_log(n.quantiles)),
                learned,
            ],
            baseline,
            neighbours,
            command_known,
        }
    }
}

/// Combines component quantiles (log seconds) by weight as a linear pool:
/// the combined chance of finishing by any time is the weighted average of
/// each component's. Each component is read as a two-piece normal through
/// its own P10, P50 and P90. When components disagree, as early in a run
/// when one expects a quick answer and another a long task, the pool keeps
/// both possibilities instead of settling on a midpoint that is wrong for
/// both. Components missing from a forecast drop out and the rest are
/// renormalized; the history baseline stands alone when nothing else has a
/// weight.
pub fn mixture(q: &[Option<[f64; 3]>; 4], weights: &[f64; 4]) -> Option<([f64; 3], [bool; 4])> {
    let mut used = [false; 4];
    let mut total = 0.0;
    for i in 0..4 {
        if q[i].is_some() && weights[i] > 0.0 {
            used[i] = true;
            total += weights[i];
        }
    }
    if total <= 0.0 {
        let h = q[Component::History as usize]?;
        let mut u = [false; 4];
        u[Component::History as usize] = true;
        return Some((h, u));
    }
    const Z90: f64 = 1.281_551_6;
    let parts: Vec<(f64, [f64; 3])> = (0..4)
        .filter(|i| used[*i])
        .map(|i| (weights[i] / total, q[i].expect("used")))
        .collect();
    let cdf = |y: f64| -> f64 {
        parts
            .iter()
            .map(|(w, v)| {
                let sd = if y < v[1] { v[1] - v[0] } else { v[2] - v[1] } / Z90;
                w * (1.0 - crate::stats::norm_sf((y - v[1]) / sd.max(1e-3)))
            })
            .sum()
    };
    let lo = parts
        .iter()
        .map(|(_, v)| v[0])
        .fold(f64::INFINITY, f64::min)
        - 1.0;
    let hi = parts
        .iter()
        .map(|(_, v)| v[2])
        .fold(f64::NEG_INFINITY, f64::max)
        + 1.0;
    let quantile = |p: f64| {
        let (mut a, mut b) = (lo, hi);
        for _ in 0..24 {
            let m = 0.5 * (a + b);
            if cdf(m) < p {
                a = m;
            } else {
                b = m;
            }
        }
        0.5 * (a + b)
    };
    Some(([quantile(0.1), quantile(0.5), quantile(0.9)], used))
}

#[derive(Debug, Clone)]
struct Sample {
    fold: usize,
    run: usize,
    elapsed: f64,
    q: [Option<[f64; 3]>; 4],
    y: f64,
}

fn distinct_runs<'a>(samples: impl Iterator<Item = &'a Sample>) -> usize {
    let mut ids: Vec<usize> = samples.map(|s| s.run).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

fn validate(sorted: &[&TrainingRun], commands: &CommandTable) -> (Vec<Sample>, usize) {
    let n = sorted.len();
    let mut samples = Vec::new();
    let mut last_train = 0;
    for fold in 0..FOLD_CUTS.len() - 1 {
        let start = (n as f64 * FOLD_CUTS[fold]).round() as usize;
        let end = (n as f64 * FOLD_CUTS[fold + 1]).round() as usize;
        if start == 0 || start >= end {
            continue;
        }
        let train = &sorted[..start];
        let now = train.last().map(|r| r.ended_at).unwrap_or_else(Utc::now);
        let labelled_train = train.iter().filter(|r| r.labelled()).count();
        let models = Models::fit(
            train,
            commands.clone(),
            now,
            labelled_train >= LEARNED_MIN_FIT_RUNS,
        );
        let block: Vec<usize> = (start..end).filter(|i| sorted[*i].labelled()).collect();
        let step = block.len().div_ceil(FOLD_MAX_RUNS).max(1);
        for &i in block.iter().step_by(step) {
            let r = sorted[i];
            let candidates = r
                .prompt
                .as_ref()
                .and_then(|p| p.sketch.as_deref())
                .map(|s| models.similar.candidates(s, r.agent))
                .unwrap_or_default();
            for t in r.checkpoints(VALIDATION_CHECKPOINTS) {
                let snap = r.snapshot_at(t);
                let p = models.predict(&r.query(&snap), now, VALIDATION_PATHS, Some(&candidates));
                samples.push(Sample {
                    fold,
                    run: i,
                    elapsed: t,
                    q: p.q,
                    y: (r.active_seconds - t).max(1.0).ln(),
                });
            }
        }
        last_train = start;
    }
    (samples, last_train)
}

/// Mean P10/P50/P90 pinball loss, in ln seconds: a proper score, so it
/// rewards an honest range as well as an accurate median.
pub fn pinball(q: [f64; 3], y: f64) -> f64 {
    [(0.1, q[0]), (0.5, q[1]), (0.9, q[2])]
        .iter()
        .map(|&(tau, v)| {
            let u = y - v;
            u * (tau - f64::from(u < 0.0))
        })
        .sum::<f64>()
        / 3.0
}

/// Pinball loss of the combined forecast, with every run counting once
/// however many moments it was scored at.
fn quantile_loss(samples: &[&Sample], weights: &[f64; 4]) -> f64 {
    let mut per_run: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for s in samples {
        *per_run.entry(s.run).or_default() += 1;
    }
    let (mut sum, mut total) = (0.0, 0.0);
    for s in samples {
        let Some((q, _)) = mixture(&s.q, weights) else {
            continue;
        };
        let w = 1.0 / per_run[&s.run] as f64;
        sum += w * pinball(q, s.y);
        total += w;
    }
    if total > 0.0 {
        sum / total
    } else {
        f64::INFINITY
    }
}

/// Weights in tenths that sum to one, in multiples of `step` tenths.
fn simplex(step: u32) -> impl Iterator<Item = [u32; 4]> {
    (0..=10).step_by(step as usize).flat_map(move |a| {
        (0..=10 - a).step_by(step as usize).flat_map(move |b| {
            (0..=10 - a - b)
                .step_by(step as usize)
                .map(move |c| [a, b, c, 10 - a - b - c])
        })
    })
}

/// Searches the weight simplex for the lowest quantile loss, in steps of 0.2
/// and then 0.1 around the best, and pulls the result toward the defaults by
/// how few runs it rests on.
fn learn_weights(samples: &[&Sample]) -> [f64; 4] {
    let present: [bool; 4] = std::array::from_fn(|i| samples.iter().any(|s| s.q[i].is_some()));
    // An even spread of at most this many forecasts decides the weights.
    let step = samples.len().div_ceil(WEIGHT_SAMPLES).max(1);
    let spread: Vec<&Sample> = samples.iter().step_by(step).copied().collect();
    let samples = &spread[..];
    let mut best = (f64::INFINITY, DEFAULT_WEIGHTS, [0u32; 4]);
    let consider = |best: &mut (f64, [f64; 4], [u32; 4]), units: [u32; 4]| {
        let w = units.map(|v| v as f64 / 10.0);
        // Weight on a model that never forecast changes nothing.
        if (0..4).any(|i| w[i] > 0.0 && !present[i]) {
            return;
        }
        let loss = quantile_loss(samples, &w);
        if loss < best.0 - 1e-9 {
            *best = (loss, w, units);
        }
    };
    simplex(2).for_each(|u| consider(&mut best, u));
    let centre = best.2;
    simplex(1)
        .filter(|u| (0..4).all(|i| u[i].abs_diff(centre[i]) <= 1))
        .for_each(|u| consider(&mut best, u));
    let runs = distinct_runs(samples.iter().copied()) as f64;
    let fitted = runs / (runs + WEIGHT_PRIOR_RUNS);
    let scale: f64 = DEFAULT_WEIGHTS.iter().sum();
    let mut w = [0.0; 4];
    for i in 0..4 {
        w[i] = fitted * best.1[i] + (1.0 - fitted) * DEFAULT_WEIGHTS[i] / scale;
    }
    w
}

fn scores(samples: &[&Sample], weights: &[f64; 4]) -> Vec<Score> {
    samples
        .iter()
        .filter_map(|s| {
            Some(Score {
                elapsed: s.elapsed,
                q: mixture(&s.q, weights)?.0,
                y: s.y,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComponentReport {
    /// Validation runs on which the component produced a forecast.
    pub validated_runs: usize,
    /// Median absolute error of its median, in log seconds.
    pub median_abs_log_error: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FitReport {
    pub trained_at: Option<DateTime<Utc>>,
    pub finished_runs: usize,
    pub timeline_runs: usize,
    pub sketch_runs: usize,
    pub validation_runs: usize,
    pub validation_samples: usize,
    pub weights_learned: bool,
    pub components: [ComponentReport; 4],
    pub holdout: Option<HoldoutReport>,
}

#[derive(Debug, Clone)]
pub struct Fitted {
    pub models: Models,
    pub weights: [f64; 4],
    pub conformal: Option<Conformal>,
    pub report: FitReport,
}

pub fn fit(data: &TrainingSet, now: DateTime<Utc>) -> Fitted {
    let commands = CommandTable::new(data.commands.iter().cloned());
    let mut sorted: Vec<&TrainingRun> = data
        .runs
        .iter()
        .filter(|r| r.active_seconds >= 1.0)
        .collect();
    sorted.sort_by_key(|r| r.ended_at);
    let labelled = sorted.iter().filter(|r| r.labelled()).count();

    let mut report = FitReport {
        trained_at: Some(now),
        finished_runs: labelled,
        timeline_runs: sorted
            .iter()
            .filter(|r| r.timeline.as_ref().is_some_and(|t| !t.is_empty()))
            .count(),
        sketch_runs: sorted
            .iter()
            .filter(|r| r.labelled() && r.prompt.as_ref().is_some_and(|p| p.sketch.is_some()))
            .count(),
        ..Default::default()
    };
    let mut weights = DEFAULT_WEIGHTS;
    let mut conformal = None;
    let mut learned_ok = false;

    if labelled >= VALIDATION_MIN_RUNS {
        let (samples, last_train) = validate(&sorted, &commands);
        let all: Vec<&Sample> = samples.iter().collect();
        report.validation_runs = distinct_runs(all.iter().copied());
        report.validation_samples = samples.len();
        for (i, c) in report.components.iter_mut().enumerate() {
            let active: Vec<&Sample> = all.iter().copied().filter(|s| s.q[i].is_some()).collect();
            c.validated_runs = distinct_runs(active.iter().copied());
            let mut e: Vec<f64> = active
                .iter()
                .map(|s| (s.y - s.q[i].expect("active")[1]).abs())
                .collect();
            c.median_abs_log_error = crate::stats::median(&mut e);
        }
        if report.validation_runs >= MIN_STACK_RUNS {
            weights = learn_weights(&all);
            report.weights_learned = true;
        }
        learned_ok = labelled >= LEARNED_MIN_RUNS
            && report.components[Component::Learned as usize].validated_runs
                >= LEARNED_MIN_VALIDATED_RUNS
            && weights[Component::Learned as usize] > 0.0;
        if !learned_ok {
            weights[Component::Learned as usize] = 0.0;
        }
        conformal = Conformal::fit(&scores(&all, &weights), report.validation_runs);
        report.holdout = holdout(&samples, last_train);
    }

    let models = Models::fit(&sorted, commands, now, learned_ok);
    if models.learned.is_none() {
        weights[Component::Learned as usize] = 0.0;
    }
    Fitted {
        models,
        weights,
        conformal,
        report,
    }
}

/// Initial-estimate accuracy on the newest validation block, with weights
/// and calibration taken only from the earlier blocks.
fn holdout(samples: &[Sample], train_runs: usize) -> Option<HoldoutReport> {
    let last = samples.iter().map(|s| s.fold).max()?;
    let early: Vec<&Sample> = samples.iter().filter(|s| s.fold < last).collect();
    let late: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.fold == last && s.elapsed == 0.0)
        .collect();
    if late.len() < 5 {
        return None;
    }
    let early_runs = distinct_runs(early.iter().copied());
    let weights = if early_runs >= MIN_STACK_RUNS {
        let mut w = learn_weights(&early);
        if w.iter().sum::<f64>() <= 0.0 {
            w = DEFAULT_WEIGHTS;
        }
        w
    } else {
        DEFAULT_WEIGHTS
    };
    let conformal = Conformal::fit(&scores(&early, &weights), early_runs);
    let mut errors = Vec::new();
    let mut covered = 0usize;
    for s in &late {
        let Some((mut q, _)) = mixture(&s.q, &weights) else {
            continue;
        };
        if let Some(c) = &conformal {
            q = c.apply(s.elapsed, q);
        }
        let actual = s.y.exp();
        errors.push((q[1].exp() - actual).abs());
        covered += usize::from(q[0] <= s.y && s.y <= q[2]);
    }
    let n = errors.len();
    Some(HoldoutReport {
        train_runs: train_runs as u32,
        test_runs: n as u32,
        median_abs_error_seconds: crate::stats::median(&mut errors)?,
        interval_coverage: covered as f64 / n as f64,
    })
}
