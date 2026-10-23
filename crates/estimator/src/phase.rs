//! Semi-Markov model of how a run moves between phases.
//!
//! A run is a sequence of segments (see [`crate::timeline`]). Each segment
//! has a phase, a tool duration, a gap until the next tool call, and an
//! outcome. The model learns, from finished runs, the probability of each
//! next phase (or of the run ending) given the current phase and whether its
//! tool call failed, and log-normal durations per phase. Remaining time is
//! the distribution of simulated continuations from the current moment.

use crate::command::CommandTable;
use crate::rng::Rng;
use crate::timeline::{RunSnapshot, Segment};
use runmarq_core::event::{AgentSource, TaskCategory};
use runmarq_core::signals::Phase;
use std::collections::HashMap;

const PHASES: usize = 7;
const END: usize = PHASES;
const STATES: usize = PHASES * 2;
const TARGETS: usize = PHASES + 1;

/// Runs with segment data needed before the model contributes.
pub const MIN_RUNS: usize = 10;
const PRIOR_STRENGTH: f64 = 4.0;
const LEVEL_STRENGTH: f64 = 20.0;
const DURATION_PRIOR_STRENGTH: f64 = 5.0;
const AGENT_DURATION_STRENGTH: f64 = 10.0;
const FAIL_PRIOR_STRENGTH: f64 = 5.0;
const MAX_SEGMENTS: usize = 400;
const MAX_SECONDS: f64 = 24.0 * 3600.0;
const FLOOR: f64 = 0.2;

type Matrix = [[f64; TARGETS]; STATES];

fn state(p: Phase, failed: bool) -> usize {
    p.index() * 2 + usize::from(failed)
}

/// Default transitions, used as a weak prior. Order: plan, explore, edit,
/// verify, execute, delegate, compact, end.
fn prior_row(from: Phase, failed: bool) -> [f64; TARGETS] {
    let mut row = match from {
        Phase::Plan => [0.0, 0.45, 0.10, 0.05, 0.15, 0.04, 0.01, 0.20],
        Phase::Explore => [0.0, 0.55, 0.20, 0.04, 0.08, 0.03, 0.01, 0.09],
        Phase::Edit => [0.0, 0.15, 0.45, 0.20, 0.08, 0.01, 0.01, 0.10],
        Phase::Verify => [0.0, 0.10, 0.15, 0.20, 0.10, 0.01, 0.01, 0.43],
        Phase::Execute => [0.0, 0.25, 0.20, 0.10, 0.30, 0.02, 0.01, 0.12],
        Phase::Delegate => [0.0, 0.20, 0.15, 0.05, 0.10, 0.30, 0.01, 0.19],
        Phase::Compact => [0.0, 0.40, 0.20, 0.10, 0.20, 0.05, 0.0, 0.05],
    };
    if failed {
        let moved = row[END] * 0.7;
        row[END] -= moved;
        match from {
            Phase::Verify => row[Phase::Edit.index()] += moved,
            Phase::Plan | Phase::Compact => row[Phase::Explore.index()] += moved,
            p => row[p.index()] += moved,
        }
    }
    row
}

fn prior_matrix() -> Matrix {
    let mut m = [[0.0; TARGETS]; STATES];
    for p in Phase::ALL {
        for f in [false, true] {
            m[state(p, f)] = prior_row(p, f);
        }
    }
    m
}

fn normalize(counts: &Matrix, parent: &Matrix, strength: f64) -> Matrix {
    let mut out = [[0.0; TARGETS]; STATES];
    for s in 0..STATES {
        let total: f64 = counts[s].iter().sum::<f64>() + strength;
        for t in 0..TARGETS {
            out[s][t] = (counts[s][t] + strength * parent[s][t]) / total;
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogNormal {
    pub mu: f64,
    pub sigma: f64,
}

impl LogNormal {
    fn of(median: f64, sigma: f64) -> LogNormal {
        LogNormal {
            mu: median.ln(),
            sigma,
        }
    }

    fn sample(&self, rng: &mut Rng) -> f64 {
        (self.mu + self.sigma * rng.normal()).exp()
    }

    /// Remaining time of a duration already `elapsed` long.
    fn sample_beyond(&self, elapsed: f64, rng: &mut Rng) -> f64 {
        if elapsed <= FLOOR {
            return self.sample(rng);
        }
        let a = (elapsed.ln() - self.mu) / self.sigma;
        if a > 8.0 {
            let hazard = a / (self.sigma * elapsed);
            return -rng.uniform().ln() / hazard;
        }
        let s = crate::stats::norm_sf(a).max(1e-300);
        let z = crate::stats::norm_isf(rng.uniform() * s);
        ((self.mu + self.sigma * z).exp() - elapsed).max(0.0)
    }
}

#[derive(Debug, Clone, Default)]
struct Moments {
    w: f64,
    wx: f64,
    wx2: f64,
}

impl Moments {
    fn add(&mut self, w: f64, x: f64) {
        self.w += w;
        self.wx += w * x;
        self.wx2 += w * x * x;
    }

    fn posterior(&self, prior: LogNormal, strength: f64) -> LogNormal {
        let mu = (strength * prior.mu + self.wx) / (strength + self.w);
        let ss = (self.wx2 - 2.0 * mu * self.wx + mu * mu * self.w).max(0.0);
        let var = (strength * prior.sigma * prior.sigma + ss) / (strength + self.w);
        LogNormal {
            mu,
            sigma: var.sqrt().clamp(0.15, 2.0),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DurationStats {
    tool: [Moments; PHASES],
    gap: [Moments; PHASES],
    fail_w: [f64; PHASES],
    calls_w: [f64; PHASES],
}

impl DurationStats {
    fn add(&mut self, s: &Segment, w: f64) {
        let i = s.phase.index();
        if s.phase != Phase::Plan && s.phase != Phase::Compact {
            self.tool[i].add(w, s.tool_secs.max(FLOOR).ln());
            self.calls_w[i] += w;
            if s.failed {
                self.fail_w[i] += w;
            }
        }
        self.gap[i].add(w, s.gap_secs().max(FLOOR).ln());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Durations {
    pub tool: [LogNormal; PHASES],
    pub gap: [LogNormal; PHASES],
    pub fail: [f64; PHASES],
}

impl Durations {
    fn prior() -> Durations {
        let tool = [
            (FLOOR, 0.3),
            (1.0, 1.0),
            (1.0, 0.8),
            (20.0, 1.2),
            (5.0, 1.3),
            (90.0, 1.0),
            (FLOOR, 0.3),
        ];
        let gap = [
            (6.0, 1.0),
            (5.0, 0.9),
            (7.0, 0.9),
            (8.0, 0.9),
            (6.0, 0.9),
            (8.0, 0.9),
            (20.0, 1.0),
        ];
        Durations {
            tool: tool.map(|(m, s)| LogNormal::of(m, s)),
            gap: gap.map(|(m, s)| LogNormal::of(m, s)),
            fail: [0.0, 0.05, 0.05, 0.3, 0.15, 0.05, 0.0],
        }
    }

    fn fit(
        stats: &DurationStats,
        parent: &Durations,
        strength: f64,
        fail_strength: f64,
    ) -> Durations {
        let mut d = parent.clone();
        for i in 0..PHASES {
            d.tool[i] = stats.tool[i].posterior(parent.tool[i], strength);
            d.gap[i] = stats.gap[i].posterior(parent.gap[i], strength);
            d.fail[i] = (stats.fail_w[i] + fail_strength * parent.fail[i])
                / (stats.calls_w[i] + fail_strength);
        }
        d
    }
}

/// One finished run's segments, as used for fitting.
#[derive(Debug, Clone)]
pub struct PhaseRun<'a> {
    pub agent: AgentSource,
    pub category: TaskCategory,
    pub segments: &'a [Segment],
    /// Whether the run ended on the agent's side, so its last segment
    /// transitions to the end.
    pub ended: bool,
    pub weight: f64,
}

#[derive(Debug, Clone)]
pub struct PhaseModel {
    global: Matrix,
    by_category: HashMap<TaskCategory, Matrix>,
    by_agent_category: HashMap<(AgentSource, TaskCategory), Matrix>,
    durations: Durations,
    by_agent: HashMap<AgentSource, Durations>,
    runs: usize,
    /// Weighted mean of ln(active seconds per segment) across runs.
    log_segment: f64,
}

/// Segments of the current run before its own pace counts fully; fewer are
/// shrunk toward the typical pace.
const PACE_STRENGTH: f64 = 8.0;
const PACE_MAX: f64 = 3.0;

impl PhaseModel {
    pub fn fit<'a>(runs: impl IntoIterator<Item = PhaseRun<'a>>) -> Option<PhaseModel> {
        let mut global = [[0.0; TARGETS]; STATES];
        let mut cat: HashMap<TaskCategory, Matrix> = HashMap::new();
        let mut agent_cat: HashMap<(AgentSource, TaskCategory), Matrix> = HashMap::new();
        let mut stats = DurationStats::default();
        let mut agent_stats: HashMap<AgentSource, DurationStats> = HashMap::new();
        let mut n = 0usize;
        let (mut pace_w, mut pace_wx) = (0.0, 0.0);

        for r in runs {
            if r.segments.is_empty() {
                continue;
            }
            n += 1;
            let first = r.segments.first().expect("not empty");
            let last = r.segments.last().expect("not empty");
            let per_segment = (last.end - first.start) / r.segments.len() as f64;
            if per_segment > 0.0 {
                pace_w += r.weight;
                pace_wx += r.weight * per_segment.ln();
            }
            let mut count = |from: usize, to: usize| {
                global[from][to] += r.weight;
                cat.entry(r.category).or_insert([[0.0; TARGETS]; STATES])[from][to] += r.weight;
                agent_cat
                    .entry((r.agent, r.category))
                    .or_insert([[0.0; TARGETS]; STATES])[from][to] += r.weight;
            };
            for w in r.segments.windows(2) {
                count(state(w[0].phase, w[0].failed), w[1].phase.index());
            }
            if r.ended {
                let last = r.segments.last().expect("not empty");
                count(state(last.phase, last.failed), END);
            }
            let a = agent_stats.entry(r.agent).or_default();
            for s in r.segments {
                stats.add(s, r.weight);
                a.add(s, r.weight);
            }
        }
        if n < MIN_RUNS {
            return None;
        }
        let global = normalize(&global, &prior_matrix(), PRIOR_STRENGTH);
        let by_category: HashMap<TaskCategory, Matrix> = cat
            .into_iter()
            .map(|(k, c)| (k, normalize(&c, &global, LEVEL_STRENGTH)))
            .collect();
        let by_agent_category = agent_cat
            .into_iter()
            .map(|((a, c), m)| {
                let parent = by_category.get(&c).unwrap_or(&global);
                ((a, c), normalize(&m, parent, LEVEL_STRENGTH))
            })
            .collect();
        let durations = Durations::fit(
            &stats,
            &Durations::prior(),
            DURATION_PRIOR_STRENGTH,
            FAIL_PRIOR_STRENGTH,
        );
        let by_agent = agent_stats
            .into_iter()
            .map(|(a, s)| {
                (
                    a,
                    Durations::fit(
                        &s,
                        &durations,
                        AGENT_DURATION_STRENGTH,
                        FAIL_PRIOR_STRENGTH * 2.0,
                    ),
                )
            })
            .collect();
        Some(PhaseModel {
            global,
            by_category,
            by_agent_category,
            durations,
            by_agent,
            runs: n,
            log_segment: if pace_w > 0.0 { pace_wx / pace_w } else { 0.0 },
        })
    }

    pub fn runs(&self) -> usize {
        self.runs
    }

    /// How much slower (above 1) or faster than usual this run moves from
    /// one tool call to the next, shrunk toward 1 while it is young.
    pub fn pace(&self, snap: &RunSnapshot) -> f64 {
        if snap.segments < 3 || snap.active_elapsed <= 0.0 || self.log_segment == 0.0 {
            return 1.0;
        }
        let n = snap.segments as f64;
        let diff = (snap.active_elapsed / n).ln() - self.log_segment;
        (diff * n / (n + PACE_STRENGTH))
            .clamp(-PACE_MAX.ln(), PACE_MAX.ln())
            .exp()
    }

    fn matrix(&self, agent: AgentSource, category: TaskCategory) -> &Matrix {
        self.by_agent_category
            .get(&(agent, category))
            .or_else(|| self.by_category.get(&category))
            .unwrap_or(&self.global)
    }

    pub fn durations(&self, agent: AgentSource) -> &Durations {
        self.by_agent.get(&agent).unwrap_or(&self.durations)
    }

    /// Probability that the run ends right after the current segment.
    pub fn end_probability(
        &self,
        agent: AgentSource,
        category: TaskCategory,
        phase: Phase,
        failed: bool,
    ) -> f64 {
        self.matrix(agent, category)[state(phase, failed)][END]
    }

    /// Simulated remaining seconds at quantiles 0.1, 0.5 and 0.9.
    #[allow(clippy::too_many_arguments)]
    pub fn remaining(
        &self,
        agent: AgentSource,
        category: TaskCategory,
        snap: &RunSnapshot,
        commands: &CommandTable,
        repository_id: Option<&str>,
        rng: &mut Rng,
        paths: usize,
    ) -> [f64; 3] {
        let m = self.matrix(agent, category);
        let d = self.durations(agent);
        let p = snap.phase.index();
        let command = snap
            .in_flight
            .as_ref()
            .and_then(|f| f.command_key.as_deref())
            .and_then(|k| commands.lookup(repository_id, k));
        let pace = self.pace(snap);

        let mut totals: Vec<f64> = Vec::with_capacity(paths);
        for _ in 0..paths {
            let mut total;
            let mut failed;
            match (&snap.in_flight, snap.since_tool_end) {
                (Some(f), _) => {
                    total = match command {
                        Some(c) => c.remaining(f.elapsed, rng.uniform()),
                        None => d.tool[p].sample_beyond(f.elapsed, rng),
                    };
                    let fail_p = command.map(|c| c.failure_rate).unwrap_or(d.fail[p]);
                    failed = rng.uniform() < fail_p;
                    total += d.gap[p].sample(rng);
                }
                (None, Some(since)) => {
                    total = d.gap[p].sample_beyond(since, rng);
                    failed = snap.failure_streak > 0;
                }
                (None, None) => {
                    total = d.gap[p].sample_beyond(snap.segment_elapsed, rng);
                    failed = false;
                }
            }
            let mut s = state(snap.phase, failed);
            for _ in 0..MAX_SEGMENTS {
                let next = categorical(&m[s], rng);
                if next == END || total > MAX_SECONDS {
                    break;
                }
                total += (d.tool[next].sample(rng) + d.gap[next].sample(rng)) * pace;
                failed = rng.uniform() < d.fail[next];
                s = next * 2 + usize::from(failed);
            }
            totals.push(total.min(MAX_SECONDS));
        }
        totals.sort_by(f64::total_cmp);
        [0.1, 0.5, 0.9].map(|q| sorted_quantile(&totals, q).max(1.0))
    }
}

fn categorical(row: &[f64; TARGETS], rng: &mut Rng) -> usize {
    let mut u = rng.uniform() * row.iter().sum::<f64>();
    for (i, p) in row.iter().enumerate() {
        if u < *p {
            return i;
        }
        u -= p;
    }
    END
}

fn sorted_quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandStat;
    use crate::timeline::InFlight;

    fn seg(phase: Phase, tool: f64, gap: f64, failed: bool) -> Segment {
        Segment {
            phase,
            start: 0.0,
            end: tool + gap,
            tool_secs: tool,
            failed,
            command_key: None,
        }
    }

    fn runs() -> Vec<Vec<Segment>> {
        (0..40)
            .map(|i| {
                let mut v = vec![
                    seg(Phase::Plan, 0.0, 5.0, false),
                    seg(Phase::Explore, 1.0, 4.0, false),
                ];
                v.push(seg(Phase::Edit, 1.0, 6.0, false));
                if i % 2 == 0 {
                    v.push(seg(Phase::Verify, 30.0, 5.0, true));
                    v.push(seg(Phase::Edit, 1.0, 6.0, false));
                }
                v.push(seg(Phase::Verify, 30.0, 10.0, false));
                v
            })
            .collect()
    }

    fn model() -> PhaseModel {
        let data = runs();
        PhaseModel::fit(data.iter().map(|s| PhaseRun {
            agent: AgentSource::ClaudeCode,
            category: TaskCategory::BugFix,
            segments: s,
            ended: true,
            weight: 1.0,
        }))
        .unwrap()
    }

    #[test]
    fn needs_enough_runs() {
        let data = runs();
        let few = PhaseModel::fit(data.iter().take(3).map(|s| PhaseRun {
            agent: AgentSource::ClaudeCode,
            category: TaskCategory::BugFix,
            segments: s,
            ended: true,
            weight: 1.0,
        }));
        assert!(few.is_none());
    }

    #[test]
    fn learns_that_passing_verification_ends_runs() {
        let m = model();
        let passed = m.end_probability(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            Phase::Verify,
            false,
        );
        let failed = m.end_probability(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            Phase::Verify,
            true,
        );
        assert!(passed > 0.8, "{passed}");
        assert!(failed < 0.1, "{failed}");
        let d = m.durations(AgentSource::ClaudeCode);
        let verify = d.tool[Phase::Verify.index()].mu.exp();
        assert!((verify - 30.0).abs() < 8.0, "{verify}");
    }

    #[test]
    fn simulation_reflects_state() {
        let m = model();
        let commands = CommandTable::default();
        let mut rng = Rng::new(1);
        let start = RunSnapshot::default();
        let at_start = m.remaining(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            &start,
            &commands,
            None,
            &mut rng,
            400,
        );
        assert!(at_start[0] <= at_start[1] && at_start[1] <= at_start[2]);
        assert!(at_start[1] > 40.0 && at_start[1] < 200.0, "{at_start:?}");

        let after_pass = RunSnapshot {
            phase: Phase::Verify,
            since_tool_end: Some(1.0),
            ..Default::default()
        };
        let near_end = m.remaining(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            &after_pass,
            &commands,
            None,
            &mut rng,
            400,
        );
        assert!(
            near_end[1] < at_start[1] / 3.0,
            "{near_end:?} vs {at_start:?}"
        );

        let after_fail = RunSnapshot {
            failure_streak: 1,
            ..after_pass.clone()
        };
        let more = m.remaining(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            &after_fail,
            &commands,
            None,
            &mut rng,
            400,
        );
        assert!(more[1] > near_end[1] * 2.0, "{more:?} vs {near_end:?}");
    }

    #[test]
    fn in_flight_commands_use_their_own_history() {
        let m = model();
        let mut stat = CommandStat::new("repo", "cmd_slow");
        for _ in 0..10 {
            stat.observe(600.0, false);
        }
        let commands = CommandTable::new([stat]);
        let snap = RunSnapshot {
            phase: Phase::Verify,
            in_flight: Some(InFlight {
                phase: Phase::Verify,
                elapsed: 100.0,
                command_key: Some("cmd_slow".into()),
            }),
            ..Default::default()
        };
        let mut rng = Rng::new(2);
        let known = m.remaining(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            &snap,
            &commands,
            Some("repo"),
            &mut rng,
            400,
        );
        assert!(known[1] > 450.0, "{known:?}");
        let unknown = m.remaining(
            AgentSource::ClaudeCode,
            TaskCategory::BugFix,
            &snap,
            &CommandTable::default(),
            Some("repo"),
            &mut rng,
            400,
        );
        assert!(unknown[1] < known[1] / 2.0, "{unknown:?}");
    }
}
