//! Hierarchical empirical-Bayes fit over local run history.
//!
//! Each finished run contributes a *residual*: log(actual active duration)
//! minus log(the cold-start baseline for its category and complexity). The
//! residual answers "how much longer or shorter than the prior was this run?",
//! which lets runs of different kinds share statistical strength.
//!
//! Residual means and spreads are estimated along a fallback chain, each level
//! shrunk toward the level above it with a pseudo-count `KAPPA`:
//!
//! ```text
//! global -> category -> agent + category -> agent + category + repository
//! ```
//!
//! With no data, every level collapses to the prior (offset 0, prior sigma).
//! With a lot of data at the specific level, that level dominates.
//!
//! Effort level and model are shared effects: they are estimated once
//! across all runs from category-centred residuals, removed before the
//! cohort fit, and added back for the run being forecast.
//!
//! Once a run is under way, the runs that ended before its current age no
//! longer describe it. [`History::survivors`] reads the remaining time
//! directly from the earlier runs that lasted longer, which the fitted
//! curve cannot do when a history mixes very short and very long runs.
//!
//! Censoring: cancelled or interrupted runs only tell us the run would have
//! lasted *at least* as long as it did. They enter through a single EM-style
//! imputation step (the conditional mean of a truncated normal) at half
//! weight. Failed runs did end, but for different reasons than successful
//! ones; they count at half weight.

use crate::priors::{category_prior, log_baseline};
use crate::stats::{norm_pdf, norm_sf};
use chrono::{DateTime, Utc};
use runmarq_core::event::{AgentSource, ComplexityBucket, TaskCategory};
use std::collections::HashMap;

/// Prior pseudo-count used at every level of the shrinkage chain.
pub const KAPPA: f64 = 4.0;
/// Recency weighting half-life. Old runs never fall below `RECENCY_FLOOR`
/// weight, which keeps estimates from swinging on a handful of recent runs.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 60.0;
pub const RECENCY_FLOOR: f64 = 0.25;
pub const SIGMA_MIN: f64 = 0.35;
pub const SIGMA_MAX: f64 = 1.6;
/// Pseudo-count for blending the survivors' remaining time with the fitted
/// curve: the survivors take over as more of them exist.
pub const SURVIVOR_KAPPA: f64 = 2.0;
const MIN_SURVIVORS: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed,
    /// Ended by the user or by ambiguity; true duration is at least the observed one.
    Censored,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalRun {
    pub agent: AgentSource,
    pub repository_id: Option<String>,
    pub category: TaskCategory,
    pub complexity: Option<ComplexityBucket>,
    pub active_seconds: f64,
    pub outcome: Outcome,
    pub ended_at: DateTime<Utc>,
    pub effort: Option<u8>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Prior,
    Global,
    Category,
    AgentCategory,
    AgentCategoryRepository,
}

impl Level {
    pub fn reason_code(self) -> &'static str {
        match self {
            Level::Prior => "cold_start_prior",
            Level::Global => "history_global",
            Level::Category => "history_category",
            Level::AgentCategory => "history_agent_category",
            Level::AgentCategoryRepository => "history_repository",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CohortFit {
    /// Residual offset in log space added to the prior baseline.
    pub offset: f64,
    pub sigma: f64,
    /// Effective (weighted) sample size at the most specific level with data.
    pub n_eff: f64,
    /// Effective sample size of the whole history.
    pub total_weight: f64,
    /// Most specific level with at least 3 effective samples.
    pub level: Level,
    /// Shared effort and model offsets included in `offset`.
    pub effort_offset: f64,
    pub model_offset: f64,
}

#[derive(Debug, Clone)]
struct Prepared {
    agent: AgentSource,
    repository_id: Option<String>,
    category: TaskCategory,
    residual: f64,
    outcome: Outcome,
    ended_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    runs: Vec<Prepared>,
    effort: HashMap<u8, f64>,
    model: HashMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CohortKey<'a> {
    pub agent: AgentSource,
    pub repository_id: Option<&'a str>,
    pub category: TaskCategory,
    pub effort: Option<u8>,
    pub model: Option<&'a str>,
}

/// Model name without a trailing date stamp, lowercased:
/// `claude-sonnet-4-5-20250929` becomes `claude-sonnet-4-5`.
pub fn model_family(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    match lower.rsplit_once(['-', '@']) {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => lower,
    }
}

fn shrunk_means<K: std::hash::Hash + Eq>(
    items: impl Iterator<Item = (K, f64, f64)>,
) -> HashMap<K, f64> {
    let mut acc: HashMap<K, (f64, f64)> = HashMap::new();
    for (k, w, r) in items {
        let e = acc.entry(k).or_default();
        e.0 += w;
        e.1 += w * r;
    }
    acc.into_iter()
        .map(|(k, (w, wr))| (k, wr / (KAPPA + w)))
        .collect()
}

#[derive(Default, Clone, Copy)]
struct Moments {
    w: f64,
    wr: f64,
    wr2: f64,
}

impl Moments {
    fn add(&mut self, w: f64, r: f64) {
        self.w += w;
        self.wr += w * r;
        self.wr2 += w * r * r;
    }
    /// Shrunk mean and variance given the parent's mean and variance.
    fn shrink(&self, parent_mean: f64, parent_var: f64) -> (f64, f64) {
        let mean = (KAPPA * parent_mean + self.wr) / (KAPPA + self.w);
        // Weighted sum of squared deviations from the shrunk mean.
        let ss = (self.wr2 - 2.0 * mean * self.wr + mean * mean * self.w).max(0.0);
        let var = (KAPPA * parent_var + ss) / (KAPPA + self.w);
        (mean, var)
    }
}

/// P10, P50 and P90 of a smoothed mixture of weighted ln-second values.
///
/// Remaining times are often bimodal (a quick answer, or a long task), and
/// one normal fitted to them puts its median in the gap between the two and
/// its P10 far above the quick group. A normal kernel on each value keeps
/// the shape; the bandwidth follows Silverman's rule and never drops below
/// the narrowest spread a single cohort may have.
fn kernel_quantiles(points: &[(f64, f64)], effective: f64) -> [f64; 3] {
    let mean: f64 = points.iter().map(|(x, w)| x * w).sum();
    let var: f64 = points.iter().map(|(x, w)| w * (x - mean).powi(2)).sum();
    let h = (1.06 * var.sqrt() * effective.powf(-0.2)).max(SIGMA_MIN);
    let cdf = |y: f64| -> f64 { points.iter().map(|(x, w)| w * norm_cdf((y - x) / h)).sum() };
    let (lo, hi) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (x, _)| {
            (a.min(*x), b.max(*x))
        });
    [0.1, 0.5, 0.9].map(|p| {
        let (mut a, mut b) = (lo - 4.0 * h, hi + 4.0 * h);
        for _ in 0..30 {
            let m = 0.5 * (a + b);
            if cdf(m) < p {
                a = m;
            } else {
                b = m;
            }
        }
        0.5 * (a + b)
    })
}

fn norm_cdf(z: f64) -> f64 {
    1.0 - norm_sf(z)
}

fn recency_weight(age_days: f64) -> f64 {
    (0.5f64.powf(age_days.max(0.0) / RECENCY_HALF_LIFE_DAYS)).max(RECENCY_FLOOR)
}

fn outcome_weight(o: Outcome) -> f64 {
    match o {
        Outcome::Completed => 1.0,
        Outcome::Failed => 0.5,
        Outcome::Censored => 0.5,
    }
}

impl History {
    pub fn new(runs: impl IntoIterator<Item = HistoricalRun>) -> History {
        let runs: Vec<HistoricalRun> = runs
            .into_iter()
            .filter(|r| r.active_seconds.is_finite() && r.active_seconds >= 1.0)
            .collect();
        let residual =
            |r: &HistoricalRun| r.active_seconds.ln() - log_baseline(r.category, r.complexity);
        let observed = || runs.iter().filter(|r| r.outcome != Outcome::Censored);

        let categories =
            shrunk_means(observed().map(|r| (r.category, outcome_weight(r.outcome), residual(r))));
        let centred =
            |r: &HistoricalRun| residual(r) - categories.get(&r.category).copied().unwrap_or(0.0);
        let effort = shrunk_means(
            observed().filter_map(|r| Some((r.effort?, outcome_weight(r.outcome), centred(r)))),
        );
        let effort_of = |r: &HistoricalRun| {
            r.effort
                .and_then(|e| effort.get(&e).copied())
                .unwrap_or(0.0)
        };
        let model = shrunk_means(observed().filter_map(|r| {
            Some((
                model_family(r.model.as_deref()?),
                outcome_weight(r.outcome),
                centred(r) - effort_of(r),
            ))
        }));
        let model_of = |r: &HistoricalRun| {
            r.model
                .as_deref()
                .and_then(|m| model.get(&model_family(m)).copied())
                .unwrap_or(0.0)
        };

        let prepared = runs
            .iter()
            .map(|r| Prepared {
                residual: residual(r) - effort_of(r) - model_of(r),
                agent: r.agent,
                repository_id: r.repository_id.clone(),
                category: r.category,
                outcome: r.outcome,
                ended_at: r.ended_at,
            })
            .collect();
        History {
            runs: prepared,
            effort,
            model,
        }
    }

    pub fn effort_offset(&self, effort: Option<u8>) -> f64 {
        effort
            .and_then(|e| self.effort.get(&e).copied())
            .unwrap_or(0.0)
    }

    pub fn model_offset(&self, model: Option<&str>) -> f64 {
        model
            .and_then(|m| self.model.get(&model_family(m)).copied())
            .unwrap_or(0.0)
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    fn weights(&self, now: DateTime<Utc>) -> Vec<f64> {
        self.runs
            .iter()
            .map(|r| {
                let age = (now - r.ended_at).num_seconds() as f64 / 86_400.0;
                recency_weight(age) * outcome_weight(r.outcome)
            })
            .collect()
    }

    /// Residuals with censored runs imputed from a global fit of the others:
    /// the conditional mean of a normal truncated at the observed residual.
    fn imputed(&self, weights: &[f64]) -> Vec<f64> {
        let mut g = Moments::default();
        for (r, w) in self.runs.iter().zip(weights) {
            if r.outcome != Outcome::Censored {
                g.add(*w, r.residual);
            }
        }
        let (g_mean, g_var) = g.shrink(0.0, 1.0);
        let g_sd = g_var.sqrt().clamp(SIGMA_MIN, SIGMA_MAX);
        self.runs
            .iter()
            .map(|r| {
                if r.outcome == Outcome::Censored {
                    let a = (r.residual - g_mean) / g_sd;
                    let tail = norm_sf(a).max(1e-12);
                    (g_mean + g_sd * norm_pdf(a) / tail).max(r.residual)
                } else {
                    r.residual
                }
            })
            .collect()
    }

    /// Remaining time read from the earlier runs that lasted longer than
    /// `elapsed`, as ln-seconds [P10, P50, P90], with their effective count.
    /// Each run is first scaled to the target's kind of task through its
    /// residual. The closest cohort with enough such runs is used, in the
    /// order of the fallback chain. `log_target` is the ln-seconds baseline
    /// of the run being forecast, including effort and model.
    pub fn survivors(
        &self,
        key: &CohortKey<'_>,
        now: DateTime<Utc>,
        elapsed: f64,
        log_target: f64,
    ) -> Option<([f64; 3], f64)> {
        if self.runs.is_empty() {
            return None;
        }
        let weights = self.weights(now);
        let residuals = self.imputed(&weights);
        // (ln remaining, weight, closeness: 0 other category .. 3 same repository)
        let mut picked: Vec<(f64, f64, u8)> = Vec::new();
        for ((r, w), res) in self.runs.iter().zip(&weights).zip(&residuals) {
            let duration = (log_target + res).exp();
            if duration <= elapsed + 1.0 {
                continue;
            }
            let closeness = if r.category != key.category {
                0
            } else if r.agent != key.agent {
                1
            } else if key.repository_id.is_some() && r.repository_id.as_deref() == key.repository_id
            {
                3
            } else {
                2
            };
            picked.push(((duration - elapsed).ln(), *w, closeness));
        }
        (0..4u8).rev().find_map(|level| {
            let set = || picked.iter().filter(|p| p.2 >= level);
            let sw: f64 = set().map(|p| p.1).sum();
            let sw2: f64 = set().map(|p| p.1 * p.1).sum();
            if sw <= 0.0 {
                return None;
            }
            let effective = sw * sw / sw2;
            if effective < MIN_SURVIVORS {
                return None;
            }
            let points: Vec<(f64, f64)> = set().map(|p| (p.0, p.1 / sw)).collect();
            Some((kernel_quantiles(&points, effective), effective))
        })
    }

    pub fn fit(&self, key: &CohortKey<'_>, now: DateTime<Utc>) -> CohortFit {
        let prior_sigma = category_prior(key.category).sigma;
        if self.runs.is_empty() {
            return CohortFit {
                offset: 0.0,
                sigma: prior_sigma,
                n_eff: 0.0,
                total_weight: 0.0,
                level: Level::Prior,
                effort_offset: 0.0,
                model_offset: 0.0,
            };
        }

        // Pass 1: a global fit on uncensored runs imputes the censored ones.
        let weights = self.weights(now);
        let residuals = self.imputed(&weights);

        // Pass 2: partition history into *disjoint* sets, from least to most
        // similar to the target. Each run is used exactly once; nesting the
        // sets instead would re-use the same runs at every level and make a
        // handful of runs look far more conclusive than they are.
        //   0: other categories, 1: same category / other agents,
        //   2: same agent + category / other repos, 3: same agent + category + repo.
        let mut sets = [Moments::default(); 4];
        for ((r, w), res) in self.runs.iter().zip(&weights).zip(&residuals) {
            let idx = if r.category != key.category {
                0
            } else if r.agent != key.agent {
                1
            } else if key.repository_id.is_some() && r.repository_id.as_deref() == key.repository_id
            {
                3
            } else {
                2
            };
            sets[idx].add(*w, *res);
        }

        let mut mean = 0.0;
        let mut var = prior_sigma * prior_sigma;
        for m in sets.iter().filter(|m| m.w > 0.0) {
            let (nm, nv) = m.shrink(mean, var);
            mean = nm;
            var = nv;
        }

        // Report the most specific level backed by at least 3 effective runs,
        // counting every run at least that similar to the target.
        let chain = [
            Level::Global,
            Level::Category,
            Level::AgentCategory,
            Level::AgentCategoryRepository,
        ];
        let mut level = Level::Prior;
        let mut n_eff = 0.0;
        for (i, lvl) in chain.iter().enumerate() {
            let cumulative: f64 = sets[i..].iter().map(|m| m.w).sum();
            if cumulative >= 3.0 {
                level = *lvl;
                n_eff = cumulative;
            }
        }
        let total_weight: f64 = sets.iter().map(|m| m.w).sum();
        if level == Level::Prior {
            n_eff = total_weight;
        }

        let effort_offset = self.effort_offset(key.effort);
        let model_offset = self.model_offset(key.model);
        CohortFit {
            offset: mean + effort_offset + model_offset,
            sigma: var.sqrt().clamp(SIGMA_MIN, SIGMA_MAX),
            n_eff,
            total_weight,
            level,
            effort_offset,
            model_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn run(cat: TaskCategory, secs: f64, outcome: Outcome, days_ago: i64) -> HistoricalRun {
        HistoricalRun {
            agent: AgentSource::ClaudeCode,
            repository_id: Some("repo".into()),
            category: cat,
            complexity: None,
            active_seconds: secs,
            outcome,
            ended_at: Utc::now() - Duration::days(days_ago),
            effort: None,
            model: None,
        }
    }

    fn key() -> CohortKey<'static> {
        CohortKey {
            agent: AgentSource::ClaudeCode,
            repository_id: Some("repo"),
            category: TaskCategory::BugFix,
            effort: None,
            model: None,
        }
    }

    #[test]
    fn survivors_keep_quick_and_long_runs_apart() {
        // Half the runs end around 40 s, half around 20 min.
        let runs = (0..30).map(|i| {
            let secs = if i % 2 == 0 { 40.0 } else { 1200.0 } * (1.0 + 0.05 * (i % 3) as f64);
            run(TaskCategory::BugFix, secs, Outcome::Completed, 1)
        });
        let h = History::new(runs);
        let target = log_baseline(TaskCategory::BugFix, None);
        let (q, _) = h.survivors(&key(), Utc::now(), 10.0, target).unwrap();
        let [p10, p50, p90] = q.map(f64::exp);
        assert!(p10 < 60.0, "the quick group sets the low end: {p10}");
        assert!(p90 > 900.0, "the long group sets the high end: {p90}");
        assert!(p10 < p50 && p50 < p90);
    }

    #[test]
    fn empty_history_is_the_prior() {
        let f = History::default().fit(&key(), Utc::now());
        assert_eq!(f.level, Level::Prior);
        assert_eq!(f.offset, 0.0);
        assert_eq!(f.sigma, category_prior(TaskCategory::BugFix).sigma);
    }

    #[test]
    fn converges_to_observed_scale() {
        // Bug fixes in this repo take ~4x the prior median, consistently.
        let target = category_prior(TaskCategory::BugFix).median_seconds * 4.0;
        let runs = (0..60).map(|i| {
            run(
                TaskCategory::BugFix,
                target * (1.0 + 0.1 * ((i % 5) as f64 - 2.0)),
                Outcome::Completed,
                1,
            )
        });
        let f = History::new(runs).fit(&key(), Utc::now());
        assert_eq!(f.level, Level::AgentCategoryRepository);
        assert!((f.offset - 4f64.ln()).abs() < 0.1, "offset {}", f.offset);
        assert!(f.sigma <= 0.45, "sigma {}", f.sigma);
    }

    #[test]
    fn few_runs_are_shrunk_toward_parent() {
        let runs = vec![run(TaskCategory::BugFix, 2000.0, Outcome::Completed, 1)];
        let f = History::new(runs).fit(&key(), Utc::now());
        let raw = 2000f64.ln() - category_prior(TaskCategory::BugFix).median_seconds.ln();
        assert!(
            f.offset > 0.0 && f.offset < raw * 0.5,
            "single run must not dominate: {}",
            f.offset
        );
        assert_eq!(
            f.level,
            Level::Prior,
            "one run is not enough to claim a history level"
        );
    }

    #[test]
    fn other_categories_inform_global_scale() {
        // The user's runs are consistently 3x the priors, in another category.
        let runs = (0..40).map(|_| {
            run(
                TaskCategory::Testing,
                category_prior(TaskCategory::Testing).median_seconds * 3.0,
                Outcome::Completed,
                1,
            )
        });
        let f = History::new(runs).fit(&key(), Utc::now());
        assert_eq!(f.level, Level::Global);
        assert!(f.offset > 0.6, "global scale carries over: {}", f.offset);
    }

    #[test]
    fn censored_runs_only_push_upward() {
        let base: Vec<_> = (0..20)
            .map(|_| run(TaskCategory::BugFix, 180.0, Outcome::Completed, 1))
            .collect();
        let without = History::new(base.clone()).fit(&key(), Utc::now());
        let mut with = base.clone();
        // Cancelled very early: almost no information, must not drag the estimate down.
        with.extend((0..20).map(|_| run(TaskCategory::BugFix, 5.0, Outcome::Censored, 1)));
        let with_early = History::new(with).fit(&key(), Utc::now());
        assert!(
            with_early.offset >= without.offset - 0.05,
            "{} vs {}",
            with_early.offset,
            without.offset
        );
        let mut late = base;
        late.extend((0..20).map(|_| run(TaskCategory::BugFix, 1800.0, Outcome::Censored, 1)));
        let with_late = History::new(late).fit(&key(), Utc::now());
        assert!(with_late.offset > without.offset + 0.3);
    }

    #[test]
    fn effort_is_a_shared_effect() {
        let base = category_prior(TaskCategory::BugFix).median_seconds;
        let testing = category_prior(TaskCategory::Testing).median_seconds;
        let mut runs = Vec::new();
        for i in 0..30 {
            let mut low = run(TaskCategory::BugFix, base, Outcome::Completed, 1);
            low.effort = Some(0);
            low.repository_id = Some(format!("r{}", i % 3));
            let mut fast_tests = run(TaskCategory::Testing, testing, Outcome::Completed, 1);
            fast_tests.effort = Some(0);
            let mut slow_tests = run(TaskCategory::Testing, testing * 4.0, Outcome::Completed, 1);
            slow_tests.effort = Some(2);
            runs.extend([low, fast_tests, slow_tests]);
        }
        let h = History::new(runs);
        assert!(h.effort_offset(Some(2)) - h.effort_offset(Some(0)) > 0.8);
        let mut k = key();
        k.effort = Some(2);
        let high = h.fit(&k, Utc::now());
        k.effort = Some(0);
        let low = h.fit(&k, Utc::now());
        assert!(
            high.offset - low.offset > 0.8,
            "{} vs {}",
            high.offset,
            low.offset
        );
        assert!(
            low.offset.abs() < 0.35,
            "bug fixes at low effort match the prior: {}",
            low.offset
        );
    }

    #[test]
    fn model_family_drops_date_stamps() {
        assert_eq!(
            model_family("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(model_family("GPT-5-Codex"), "gpt-5-codex");
        assert_eq!(model_family("claude-opus-4-1@20250805"), "claude-opus-4-1");
    }

    #[test]
    fn old_runs_count_less_but_not_zero() {
        assert!((recency_weight(0.0) - 1.0).abs() < 1e-9);
        assert!((recency_weight(60.0) - 0.5).abs() < 1e-9);
        assert_eq!(recency_weight(10_000.0), RECENCY_FLOOR);
    }
}
