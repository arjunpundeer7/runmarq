//! The description of a run at one moment that every model reads, and its
//! numeric encoding for the learned model. Training builds the same
//! [`Query`] from stored events that live forecasting builds from the open
//! run, so features mean the same thing in both.

use crate::command::CommandTable;
use crate::priors::log_baseline;
use crate::timeline::RunSnapshot;
use runmarq_core::event::{AgentSource, PromptFeatures, TaskCategory};
use runmarq_core::signals::{effort_rank, Phase, RunSignals};

#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    /// Seeds the simulation so a run at one moment always gets the same forecast.
    pub seed: &'a str,
    pub agent: AgentSource,
    pub repository_id: Option<&'a str>,
    pub prompt: Option<&'a PromptFeatures>,
    pub effort: Option<&'a str>,
    pub model: Option<&'a str>,
    pub snapshot: &'a RunSnapshot,
    pub signals: Option<&'a RunSignals>,
    pub repo_files: Option<u32>,
}

impl Query<'_> {
    pub fn category(&self) -> TaskCategory {
        self.prompt
            .map(|p| p.category)
            .unwrap_or(TaskCategory::Unknown)
    }

    pub fn elapsed(&self) -> f64 {
        self.snapshot.active_elapsed
    }
}

pub const NAMES: &[&str] = &[
    "elapsed",
    "prior_baseline",
    "cat_small_edit",
    "cat_bug_fix",
    "cat_feature",
    "cat_ui",
    "cat_refactoring",
    "cat_testing",
    "cat_dependencies",
    "cat_analysis",
    "cat_research",
    "cat_migration",
    "cat_multi_step",
    "cat_unknown",
    "agent_claude_code",
    "agent_codex",
    "agent_cursor",
    "prompt_known",
    "complexity",
    "words",
    "list_items",
    "file_mentions",
    "mentions_tests",
    "effort",
    "tool_calls",
    "tool_failures",
    "failure_streak",
    "test_runs",
    "test_failure_streak",
    "last_test_passed",
    "edits_since_test",
    "subagents",
    "plan_total",
    "plan_fraction",
    "segments",
    "verify_failures",
    "compactions",
    "phase_plan",
    "phase_explore",
    "phase_edit",
    "phase_verify",
    "phase_execute",
    "phase_delegate",
    "phase_compact",
    "segment_elapsed",
    "tool_elapsed",
    "since_tool_end",
    "last_verify",
    "command_expected",
    "command_samples",
    "turns",
    "output_tokens",
    "output_rate",
    "context_tokens",
    "context_fraction",
    "transcript_compactions",
    "edit_lines",
    "diff_files",
    "diff_lines",
    "repo_files",
];

pub const COUNT: usize = 60;

const MISSING: f32 = -1.0;

fn ln1p(v: f64) -> f32 {
    v.max(0.0).ln_1p() as f32
}

pub fn encode(q: &Query<'_>, commands: &CommandTable) -> Vec<f32> {
    let s = q.snapshot;
    let mut x: Vec<f32> = Vec::with_capacity(COUNT);
    let category = q.category();
    x.push(ln1p(s.active_elapsed));
    x.push(log_baseline(category, q.prompt.map(|p| p.complexity)) as f32);
    for c in TaskCategory::ALL {
        x.push(f32::from(u8::from(c == category)));
    }
    for a in AgentSource::ALL {
        x.push(f32::from(u8::from(a == q.agent)));
    }
    match q.prompt {
        Some(p) => {
            x.push(1.0);
            x.push(p.complexity_score);
            x.push(ln1p(p.word_count as f64));
            x.push(p.list_items.min(100) as f32);
            x.push(p.file_mentions.min(100) as f32);
            x.push(f32::from(u8::from(p.mentions_tests)));
        }
        None => {
            x.push(0.0);
            x.extend([MISSING; 5]);
        }
    }
    x.push(effort_rank(q.effort).map(f32::from).unwrap_or(MISSING));

    x.push(ln1p(s.tool_calls as f64));
    x.push(ln1p(s.tool_failures as f64));
    x.push(s.failure_streak.min(50) as f32);
    x.push(s.test_runs.min(100) as f32);
    x.push(s.test_failure_streak.min(50) as f32);
    x.push(f32::from(u8::from(s.last_test_passed)));
    x.push(s.edits_since_last_test.min(200) as f32);
    x.push(s.active_subagents.min(20) as f32);
    match &s.plan {
        Some(p) if p.total_steps > 0 => {
            x.push(p.total_steps.min(100) as f32);
            x.push((p.completed_steps as f32 / p.total_steps as f32).min(1.0));
        }
        _ => x.extend([MISSING, MISSING]),
    }
    x.push(ln1p(s.segments as f64));
    x.push(s.verify_failures.min(50) as f32);
    x.push(s.compactions.min(20) as f32);
    for p in Phase::ALL {
        x.push(f32::from(u8::from(p == s.phase)));
    }
    x.push(ln1p(s.segment_elapsed));
    x.push(
        s.in_flight
            .as_ref()
            .map(|f| ln1p(f.elapsed))
            .unwrap_or(MISSING),
    );
    x.push(s.since_tool_end.map(ln1p).unwrap_or(MISSING));
    x.push(match s.last_verify_failed {
        None => MISSING,
        Some(false) => 0.0,
        Some(true) => 1.0,
    });
    let command = s.in_flight.as_ref().and_then(|f| {
        Some((
            f,
            commands.lookup(q.repository_id, f.command_key.as_deref()?)?,
        ))
    });
    match command {
        Some((f, c)) => {
            x.push(ln1p(c.remaining(f.elapsed, 0.5)));
            x.push(ln1p(c.samples as f64));
        }
        None => x.extend([MISSING, MISSING]),
    }

    match q.signals {
        Some(g) if g.has_transcript_data() => {
            x.push(ln1p(g.turns as f64));
            x.push(ln1p(g.output_tokens as f64));
            let minutes = (s.active_elapsed / 60.0).max(0.5);
            x.push(ln1p(g.output_tokens as f64 / minutes));
            x.push(ln1p(g.context_tokens as f64));
            x.push(match g.context_window {
                Some(w) if w > 0 => (g.context_tokens as f64 / w as f64).min(2.0) as f32,
                _ => MISSING,
            });
            x.push(g.compactions.min(20) as f32);
        }
        _ => x.extend([MISSING; 6]),
    }
    let edit_lines = q.signals.map(|g| g.edit_lines).unwrap_or(0);
    x.push(ln1p(edit_lines as f64));
    x.push(
        q.signals
            .and_then(|g| g.diff_files)
            .map(|v| ln1p(v as f64))
            .unwrap_or(MISSING),
    );
    x.push(
        q.signals
            .and_then(|g| g.diff_lines)
            .map(|v| ln1p(v as f64))
            .unwrap_or(MISSING),
    );
    x.push(q.repo_files.map(|v| ln1p(v as f64)).unwrap_or(MISSING));
    debug_assert_eq!(x.len(), COUNT);
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_encoding() {
        assert_eq!(NAMES.len(), COUNT);
        let snap = RunSnapshot::default();
        let q = Query {
            seed: "r",
            agent: AgentSource::Codex,
            repository_id: None,
            prompt: None,
            effort: Some("high"),
            model: None,
            snapshot: &snap,
            signals: None,
            repo_files: Some(1200),
        };
        let x = encode(&q, &CommandTable::default());
        assert_eq!(x.len(), COUNT);
        assert_eq!(
            x[NAMES.iter().position(|n| *n == "agent_codex").unwrap()],
            1.0
        );
        assert_eq!(x[NAMES.iter().position(|n| *n == "effort").unwrap()], 2.0);
        assert_eq!(
            x[NAMES.iter().position(|n| *n == "phase_plan").unwrap()],
            1.0
        );
        assert!(x.iter().all(|v| v.is_finite()));
    }
}
