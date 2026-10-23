//! Nearest finished runs by prompt sketch.
//!
//! Sketches are salted, hashed term vectors (see [`crate::classify::sketch`]),
//! so similarity only says which earlier prompts used similar words. The
//! durations of the most similar runs that outlasted the current one give a
//! distribution of remaining time.

use crate::classify::sketch_similarity;
use runmarq_core::event::AgentSource;

pub const MIN_SIMILARITY: f32 = 0.3;
const CANDIDATES: usize = 60;
const NEIGHBOURS: usize = 20;
const MIN_EFFECTIVE: f64 = 3.0;
const OTHER_AGENT_WEIGHT: f64 = 0.4;
const Z90: f64 = 1.281_551_565_5;

#[derive(Debug, Clone)]
pub struct SimilarRun {
    pub sketch: Vec<i8>,
    pub agent: AgentSource,
    pub active_seconds: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SimilarIndex {
    runs: Vec<SimilarRun>,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    duration: f64,
    weight: f64,
    similarity: f64,
}

/// Runs similar to one prompt, most similar first.
#[derive(Debug, Clone, Default)]
pub struct Candidates(Vec<Candidate>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbours {
    /// Remaining seconds at quantiles 0.1, 0.5 and 0.9.
    pub quantiles: [f64; 3],
    pub effective: f64,
    pub mean_similarity: f64,
    pub count: usize,
}

impl SimilarIndex {
    pub fn new(runs: impl IntoIterator<Item = SimilarRun>) -> SimilarIndex {
        SimilarIndex {
            runs: runs
                .into_iter()
                .filter(|r| r.active_seconds >= 1.0 && r.sketch.iter().any(|v| *v != 0))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Scores every indexed run against `sketch` once; elapsed time is
    /// applied afterwards, so one prompt can be queried at many moments.
    pub fn candidates(&self, sketch: &[i8], agent: AgentSource) -> Candidates {
        let mut scored: Vec<(f32, usize)> = self
            .runs
            .iter()
            .enumerate()
            .map(|(i, r)| (sketch_similarity(sketch, &r.sketch), i))
            .filter(|(s, _)| *s >= MIN_SIMILARITY)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(CANDIDATES);
        Candidates(
            scored
                .into_iter()
                .map(|(sim, i)| {
                    let r = &self.runs[i];
                    let k = ((sim - MIN_SIMILARITY) / (1.0 - MIN_SIMILARITY)) as f64;
                    let agent_w = if r.agent == agent {
                        1.0
                    } else {
                        OTHER_AGENT_WEIGHT
                    };
                    Candidate {
                        duration: r.active_seconds,
                        weight: (0.05 + k * k) * r.weight * agent_w,
                        similarity: sim as f64,
                    }
                })
                .collect(),
        )
    }

    pub fn query(&self, sketch: &[i8], agent: AgentSource, elapsed: f64) -> Option<Neighbours> {
        self.candidates(sketch, agent).remaining(elapsed)
    }
}

impl Candidates {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Remaining time among similar runs that lasted longer than `elapsed`.
    pub fn remaining(&self, elapsed: f64) -> Option<Neighbours> {
        let survivors: Vec<&Candidate> = self
            .0
            .iter()
            .filter(|c| c.duration > elapsed + 1.0)
            .take(NEIGHBOURS)
            .collect();
        let sw: f64 = survivors.iter().map(|c| c.weight).sum();
        let sw2: f64 = survivors.iter().map(|c| c.weight * c.weight).sum();
        if sw <= 0.0 {
            return None;
        }
        let effective = sw * sw / sw2;
        if effective < MIN_EFFECTIVE {
            return None;
        }
        let log = |c: &Candidate| (c.duration - elapsed).ln();
        let mean = survivors.iter().map(|c| log(c) * c.weight).sum::<f64>() / sw;
        let var = survivors
            .iter()
            .map(|c| c.weight * (log(c) - mean).powi(2))
            .sum::<f64>()
            / sw;
        let sd = var.sqrt().max(0.25) * (1.0 + 1.0 / effective).sqrt();
        Some(Neighbours {
            quantiles: [(mean - Z90 * sd).exp(), mean.exp(), (mean + Z90 * sd).exp()],
            effective,
            mean_similarity: survivors.iter().map(|c| c.similarity).sum::<f64>()
                / survivors.len() as f64,
            count: survivors.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::sketch;

    fn index() -> SimilarIndex {
        let salt = b"s";
        let mut runs = Vec::new();
        for i in 0..8 {
            runs.push(SimilarRun {
                sketch: sketch(
                    &format!("migrate the billing database schema step {i}"),
                    salt,
                ),
                agent: AgentSource::ClaudeCode,
                active_seconds: 1800.0 + 60.0 * i as f64,
                weight: 1.0,
            });
            runs.push(SimilarRun {
                sketch: sketch(&format!("fix typo in the readme heading {i}"), salt),
                agent: AgentSource::ClaudeCode,
                active_seconds: 30.0 + i as f64,
                weight: 1.0,
            });
        }
        SimilarIndex::new(runs)
    }

    #[test]
    fn similar_prompts_predict_similar_durations() {
        let idx = index();
        let q = sketch("migrate billing database schema", b"s");
        let n = idx.query(&q, AgentSource::ClaudeCode, 0.0).unwrap();
        assert!(n.quantiles[1] > 1500.0 && n.quantiles[1] < 2300.0, "{n:?}");
        assert!(n.mean_similarity > 0.5);
        let t = sketch("fix the typo in readme", b"s");
        let m = idx.query(&t, AgentSource::ClaudeCode, 0.0).unwrap();
        assert!(m.quantiles[1] < 60.0, "{m:?}");
    }

    #[test]
    fn only_runs_that_outlasted_elapsed_count() {
        let idx = index();
        let t = sketch("fix the typo in readme", b"s");
        assert!(idx.query(&t, AgentSource::ClaudeCode, 120.0).is_none());
        let unrelated = sketch("translate the onboarding emails into german", b"s");
        assert!(idx
            .query(&unrelated, AgentSource::ClaudeCode, 0.0)
            .is_none());
    }
}
