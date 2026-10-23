//! Split-conformal calibration of the P10–P90 interval.
//!
//! Scores come from forecasts made for runs the models had not seen
//! (temporal validation, see [`crate::training`]). For each, the lower score
//! is how far the outcome fell below P10 and the upper score how far it rose
//! above P90, in log seconds. Shifting each bound by the finite-sample
//! `(1 - alpha/2)` quantile of its scores gives an interval that covers at
//! least the target share of outcomes, provided new runs resemble the
//! scored ones. Scores are kept per forecast-age bucket (Mondrian
//! conformal) when a bucket has enough of them, because early and late
//! forecasts err differently.

pub const TARGET_COVERAGE: f64 = 0.8;
pub const MIN_SCORES: usize = 40;
pub const MIN_RUNS: usize = 20;
const BUCKET_MIN: usize = 30;
const BUCKET_EDGES: [f64; 2] = [60.0, 300.0];
const MEDIAN_SHRINK: f64 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub elapsed: f64,
    /// Forecast quantiles and outcome, all as ln(seconds remaining).
    pub q: [f64; 3],
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Adjust {
    lo: f64,
    mid: f64,
    hi: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conformal {
    pooled: Adjust,
    buckets: [Option<Adjust>; 3],
    pub scores: usize,
    pub runs: usize,
}

fn bucket(elapsed: f64) -> usize {
    BUCKET_EDGES.iter().filter(|e| elapsed >= **e).count()
}

/// The `ceil((n + 1) * level)`-th smallest value, or the largest when that
/// rank exceeds `n`.
fn conformal_quantile(mut v: Vec<f64>, level: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let k = (((n + 1) as f64) * level).ceil() as usize;
    v[k.clamp(1, n) - 1]
}

fn adjust(scores: &[&Score]) -> Adjust {
    let tail = 1.0 - (1.0 - TARGET_COVERAGE) / 2.0;
    let lo = conformal_quantile(scores.iter().map(|s| s.q[0] - s.y).collect(), tail);
    let hi = conformal_quantile(scores.iter().map(|s| s.y - s.q[2]).collect(), tail);
    let mut d: Vec<f64> = scores.iter().map(|s| s.y - s.q[1]).collect();
    let n = d.len() as f64;
    let mid = crate::stats::median(&mut d).unwrap_or(0.0) * n / (n + MEDIAN_SHRINK);
    Adjust { lo, mid, hi }
}

impl Conformal {
    pub fn fit(scores: &[Score], runs: usize) -> Option<Conformal> {
        if scores.len() < MIN_SCORES || runs < MIN_RUNS {
            return None;
        }
        let all: Vec<&Score> = scores.iter().collect();
        let mut buckets = [None; 3];
        for (b, slot) in buckets.iter_mut().enumerate() {
            let sub: Vec<&Score> = scores.iter().filter(|s| bucket(s.elapsed) == b).collect();
            if sub.len() >= BUCKET_MIN {
                *slot = Some(adjust(&sub));
            }
        }
        Some(Conformal {
            pooled: adjust(&all),
            buckets,
            scores: scores.len(),
            runs,
        })
    }

    /// Calibrated quantiles, in log seconds.
    pub fn apply(&self, elapsed: f64, q: [f64; 3]) -> [f64; 3] {
        let a = self.buckets[bucket(elapsed)].unwrap_or(self.pooled);
        let lo = q[0] - a.lo;
        let hi = q[2] + a.hi;
        let (lo, hi) = if lo <= hi {
            (lo, hi)
        } else {
            ((lo + hi) / 2.0, (lo + hi) / 2.0)
        };
        let mid = (q[1] + a.mid).clamp(lo, hi);
        [lo, mid, hi]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn scores(n: usize, true_sd: f64, model_sd: f64, seed: u64) -> Vec<Score> {
        let mut rng = Rng::new(seed);
        (0..n)
            .map(|i| {
                let m = 5.0;
                let w = 1.281_551_565_5 * model_sd;
                Score {
                    elapsed: (i % 400) as f64,
                    q: [m - w, m, m + w],
                    y: m + true_sd * rng.normal(),
                }
            })
            .collect()
    }

    fn coverage(c: &Conformal, test: &[Score]) -> f64 {
        test.iter()
            .filter(|s| {
                let q = c.apply(s.elapsed, s.q);
                q[0] <= s.y && s.y <= q[2]
            })
            .count() as f64
            / test.len() as f64
    }

    #[test]
    fn overconfident_intervals_are_widened() {
        let cal = scores(400, 1.0, 0.4, 1);
        let c = Conformal::fit(&cal, 100).unwrap();
        let test = scores(4000, 1.0, 0.4, 2);
        let raw = test
            .iter()
            .filter(|s| s.q[0] <= s.y && s.y <= s.q[2])
            .count() as f64
            / 4000.0;
        let cov = coverage(&c, &test);
        assert!(raw < 0.5, "raw {raw}");
        assert!((0.76..0.88).contains(&cov), "calibrated {cov}");
    }

    #[test]
    fn overly_wide_intervals_are_narrowed() {
        let cal = scores(400, 0.3, 1.0, 3);
        let c = Conformal::fit(&cal, 100).unwrap();
        let q = c.apply(0.0, cal[0].q);
        assert!(q[2] - q[0] < cal[0].q[2] - cal[0].q[0]);
        let cov = coverage(&c, &scores(4000, 0.3, 1.0, 4));
        assert!((0.76..0.88).contains(&cov), "calibrated {cov}");
    }

    #[test]
    fn needs_enough_scores_and_runs() {
        assert!(Conformal::fit(&scores(30, 1.0, 1.0, 5), 30).is_none());
        assert!(Conformal::fit(&scores(400, 1.0, 1.0, 5), 5).is_none());
    }
}
