//! Learned durations of individual shell commands, keyed by a salted hash
//! of the command's normalized form (for example `cargo test`).

use crate::stats::{norm_isf, norm_sf};
use std::collections::HashMap;

pub const PRIOR_SIGMA: f64 = 0.6;
const PRIOR_STRENGTH: f64 = 3.0;
pub const MIN_SAMPLES: u32 = 3;
const FLOOR_SECONDS: f64 = 0.2;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandStat {
    pub repository_id: String,
    pub command_key: String,
    pub samples: u32,
    /// Running mean and sum of squared deviations of ln(seconds).
    pub log_mean: f64,
    pub log_m2: f64,
    pub failures: u32,
}

impl CommandStat {
    pub fn new(repository_id: &str, command_key: &str) -> CommandStat {
        CommandStat {
            repository_id: repository_id.into(),
            command_key: command_key.into(),
            samples: 0,
            log_mean: 0.0,
            log_m2: 0.0,
            failures: 0,
        }
    }

    /// Welford update with one observed duration.
    pub fn observe(&mut self, seconds: f64, failed: bool) {
        let x = seconds.max(FLOOR_SECONDS).ln();
        self.samples += 1;
        let d = x - self.log_mean;
        self.log_mean += d / self.samples as f64;
        self.log_m2 += d * (x - self.log_mean);
        if failed {
            self.failures += 1;
        }
    }

    pub fn sigma(&self) -> f64 {
        let n = self.samples as f64;
        let sample_var = if self.samples > 1 {
            self.log_m2 / (n - 1.0)
        } else {
            0.0
        };
        let var = (PRIOR_STRENGTH * PRIOR_SIGMA * PRIOR_SIGMA + (n - 1.0).max(0.0) * sample_var)
            / (PRIOR_STRENGTH + (n - 1.0).max(0.0));
        var.sqrt().clamp(0.1, 2.0)
    }

    fn merge(&mut self, o: &CommandStat) {
        let (na, nb) = (self.samples as f64, o.samples as f64);
        let n = na + nb;
        if nb == 0.0 {
            return;
        }
        let d = o.log_mean - self.log_mean;
        self.log_mean += d * nb / n;
        self.log_m2 += o.log_m2 + d * d * na * nb / n;
        self.samples += o.samples;
        self.failures += o.failures;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommandDuration {
    pub mu: f64,
    pub sigma: f64,
    pub samples: u32,
    pub failure_rate: f64,
}

impl CommandDuration {
    /// Quantile `q` of the remaining duration given `elapsed` seconds so far.
    pub fn remaining(&self, elapsed: f64, q: f64) -> f64 {
        let q = q.clamp(1e-6, 1.0 - 1e-6);
        let a = (elapsed.max(FLOOR_SECONDS).ln() - self.mu) / self.sigma;
        let s = norm_sf(a).max(1e-12);
        let z = norm_isf((1.0 - q) * s);
        ((self.mu + self.sigma * z).exp() - elapsed).max(0.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandTable {
    by_repo: HashMap<(String, String), CommandStat>,
    by_key: HashMap<String, CommandStat>,
}

impl CommandTable {
    pub fn new(stats: impl IntoIterator<Item = CommandStat>) -> CommandTable {
        let mut t = CommandTable::default();
        for s in stats {
            t.by_key
                .entry(s.command_key.clone())
                .or_insert_with(|| CommandStat::new("", &s.command_key))
                .merge(&s);
            t.by_repo
                .insert((s.repository_id.clone(), s.command_key.clone()), s);
        }
        t
    }

    pub fn len(&self) -> usize {
        self.by_repo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_repo.is_empty()
    }

    /// Records one finished command, as the collector does in storage.
    pub fn observe(&mut self, repository_id: Option<&str>, key: &str, seconds: f64, failed: bool) {
        let repo = repository_id.unwrap_or("");
        self.by_repo
            .entry((repo.to_string(), key.to_string()))
            .or_insert_with(|| CommandStat::new(repo, key))
            .observe(seconds, failed);
        self.by_key
            .entry(key.to_string())
            .or_insert_with(|| CommandStat::new("", key))
            .observe(seconds, failed);
    }

    pub fn lookup(&self, repository_id: Option<&str>, key: &str) -> Option<CommandDuration> {
        let local = self
            .by_repo
            .get(&(repository_id.unwrap_or("").to_string(), key.to_string()))
            .filter(|s| s.samples >= MIN_SAMPLES);
        let s = local.or_else(|| self.by_key.get(key).filter(|s| s.samples >= MIN_SAMPLES))?;
        Some(CommandDuration {
            mu: s.log_mean,
            sigma: s.sigma(),
            samples: s.samples,
            failure_rate: (s.failures as f64 + 0.5) / (s.samples as f64 + 2.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welford_matches_batch_statistics() {
        let xs = [10.0, 12.0, 9.0, 30.0, 11.0];
        let mut s = CommandStat::new("r", "cmd_a");
        for x in xs {
            s.observe(x, false);
        }
        let logs: Vec<f64> = xs.iter().map(|x: &f64| x.ln()).collect();
        let mean = logs.iter().sum::<f64>() / 5.0;
        let m2: f64 = logs.iter().map(|l| (l - mean).powi(2)).sum();
        assert!((s.log_mean - mean).abs() < 1e-12);
        assert!((s.log_m2 - m2).abs() < 1e-9);
    }

    #[test]
    fn lookup_prefers_the_repository_and_needs_samples() {
        let mut local = CommandStat::new("r1", "cmd_t");
        let mut other = CommandStat::new("r2", "cmd_t");
        for _ in 0..5 {
            local.observe(100.0, false);
            other.observe(10.0, true);
        }
        let mut few = CommandStat::new("r3", "cmd_x");
        few.observe(5.0, false);
        let t = CommandTable::new([local, other, few]);
        let d = t.lookup(Some("r1"), "cmd_t").unwrap();
        assert!((d.mu.exp() - 100.0).abs() < 1e-6);
        let pooled = t.lookup(Some("elsewhere"), "cmd_t").unwrap();
        assert!(pooled.mu.exp() > 10.0 && pooled.mu.exp() < 100.0);
        assert!(t.lookup(Some("r3"), "cmd_x").is_none());
        assert!(d.failure_rate < 0.2 && pooled.failure_rate > 0.3);
    }

    #[test]
    fn remaining_is_conditioned_on_elapsed() {
        let d = CommandDuration {
            mu: 60f64.ln(),
            sigma: 0.3,
            samples: 10,
            failure_rate: 0.1,
        };
        let fresh = d.remaining(0.0, 0.5);
        assert!((fresh - 60.0).abs() < 1.0);
        let late = d.remaining(120.0, 0.5);
        assert!(late > 0.0 && late < 60.0);
        assert!(d.remaining(30.0, 0.1) <= d.remaining(30.0, 0.9));
    }
}
