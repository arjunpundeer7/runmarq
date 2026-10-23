//! Explained adjustments of the history baseline from what has happened in
//! the run so far.
//!
//! The baseline conditions only on elapsed active time. These factors scale
//! its remaining-time quantiles, each tied to a reason code so the UI can
//! say why an estimate moved. The phase and learned models read the same
//! state directly, so these factors apply to the baseline alone.

use crate::timeline::RunSnapshot;

pub const TEST_FAILURE_FACTOR: f64 = 1.15;
pub const REPEATED_TEST_FAILURE_STEP: f64 = 0.3;
pub const REPEATED_TEST_FAILURE_CAP: f64 = 2.2;
pub const TOOL_FAILURE_FACTOR: f64 = 1.25;
pub const TESTS_PASSING_FACTOR: f64 = 0.6;
pub const SUBAGENT_FACTOR: f64 = 1.15;
pub const PLAN_COMPLETE_FACTOR: f64 = 0.4;
/// Active seconds with no new tool after the last one ended before a run
/// that checks out counts as finishing: the agent is writing, not working.
pub const FINISHING_QUIET_SECONDS: f64 = 20.0;

/// Scales `remaining` ([p10, p50, p90] in seconds) by the evidence in `s`.
pub fn adjust(s: &RunSnapshot, remaining: [f64; 3]) -> ([f64; 3], Vec<&'static str>) {
    let mut factor = 1.0;
    let mut reasons = Vec::new();

    if s.test_failure_streak >= 2 {
        let f = (1.0 + REPEATED_TEST_FAILURE_STEP * (s.test_failure_streak - 1) as f64)
            .min(REPEATED_TEST_FAILURE_CAP);
        factor *= f;
        reasons.push("repeated_test_failures");
    } else if s.test_failure_streak == 1 {
        factor *= TEST_FAILURE_FACTOR;
        reasons.push("test_failure");
    }

    if s.failure_streak >= 3 && s.test_failure_streak < 2 {
        factor *= TOOL_FAILURE_FACTOR;
        reasons.push("tool_failures");
    }

    if s.test_runs > 0
        && s.last_test_passed
        && s.edits_since_last_test == 0
        && s.test_failure_streak == 0
    {
        factor *= TESTS_PASSING_FACTOR;
        reasons.push("tests_passing");
    }

    if s.active_subagents > 0 {
        factor *= SUBAGENT_FACTOR;
        reasons.push("subagents_active");
    }

    let mut out = remaining.map(|q| q * factor);

    if s.plan
        .as_ref()
        .is_some_and(|p| p.total_steps >= 2 && p.completed_steps >= p.total_steps)
    {
        out = out.map(|q| q * PLAN_COMPLETE_FACTOR);
        reasons.push("plan_complete");
    }

    (out, reasons)
}

/// Remaining time implied by the pace of completed plan steps, as
/// [p10, p50, p90] seconds, and how much it should count against the models.
pub fn plan_target(s: &RunSnapshot) -> Option<([f64; 3], f64)> {
    let plan = s.plan.as_ref()?;
    if plan.total_steps < 2
        || plan.completed_steps == 0
        || plan.completed_steps >= plan.total_steps
        || s.active_elapsed < 10.0
    {
        return None;
    }
    let per_step = s.active_elapsed / plan.completed_steps as f64;
    let left = per_step * (plan.total_steps - plan.completed_steps) as f64;
    let w = (0.25 + 0.1 * plan.completed_steps as f64).min(0.7);
    Some(([0.5, 1.0, 2.0].map(|k| (left * k).max(1.0)), w))
}

/// Signs that the agent is wrapping up: its plan is done, or its tests pass
/// with no edits since, and it has not started a tool for a few seconds.
pub fn finishing(s: &RunSnapshot) -> bool {
    if s.in_flight.is_some() || s.segments < 2 {
        return false;
    }
    let quiet = s
        .since_tool_end
        .is_some_and(|t| t >= FINISHING_QUIET_SECONDS);
    let plan_done = s
        .plan
        .as_ref()
        .is_some_and(|p| p.total_steps >= 2 && p.completed_steps >= p.total_steps);
    let verified = s.test_runs > 0
        && s.last_test_passed
        && s.edits_since_last_test == 0
        && s.test_failure_streak == 0;
    quiet && (plan_done || verified)
}

/// Plan-based progress fraction, only when a genuine plan exists.
pub fn plan_progress(s: &RunSnapshot) -> Option<f64> {
    s.plan
        .as_ref()
        .filter(|p| p.total_steps >= 2)
        .map(|p| p.completed_steps as f64 / p.total_steps as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runmarq_core::event::PlanSnapshot;

    const BASE: [f64; 3] = [60.0, 120.0, 300.0];

    #[test]
    fn no_evidence_is_identity() {
        let (out, reasons) = adjust(&RunSnapshot::default(), BASE);
        assert_eq!(out, BASE);
        assert!(reasons.is_empty());
    }

    #[test]
    fn repeated_test_failures_increase_estimate() {
        let s = RunSnapshot {
            test_runs: 3,
            test_failure_streak: 3,
            ..Default::default()
        };
        let (out, reasons) = adjust(&s, BASE);
        assert!(out[1] > BASE[1] * 1.5);
        assert_eq!(reasons, vec!["repeated_test_failures"]);
        let s = RunSnapshot {
            test_runs: 20,
            test_failure_streak: 20,
            ..Default::default()
        };
        let (out, _) = adjust(&s, BASE);
        assert!(
            (out[1] / BASE[1] - REPEATED_TEST_FAILURE_CAP).abs() < 1e-9,
            "capped"
        );
    }

    #[test]
    fn passing_tests_after_edits_shrink_estimate() {
        let s = RunSnapshot {
            test_runs: 2,
            last_test_passed: true,
            ..Default::default()
        };
        let (out, reasons) = adjust(&s, BASE);
        assert!(out[1] < BASE[1]);
        assert_eq!(reasons, vec!["tests_passing"]);
        let s = RunSnapshot {
            edits_since_last_test: 3,
            ..s
        };
        assert_eq!(
            adjust(&s, BASE).0,
            BASE,
            "more edits since the test: no wrap-up signal"
        );
    }

    #[test]
    fn plan_pace_gives_a_target() {
        let s = RunSnapshot {
            active_elapsed: 300.0,
            plan: Some(PlanSnapshot {
                total_steps: 4,
                completed_steps: 3,
            }),
            ..Default::default()
        };
        let (target, w) = plan_target(&s).unwrap();
        assert_eq!(target, [50.0, 100.0, 200.0]);
        assert!(w > 0.5 && w <= 0.7);
        assert_eq!(plan_progress(&s), Some(0.75));
        assert!(
            adjust(&s, BASE).1.is_empty(),
            "the target applies to the final forecast"
        );
    }

    #[test]
    fn finishing_needs_checked_work_and_quiet() {
        let s = RunSnapshot {
            segments: 6,
            test_runs: 2,
            last_test_passed: true,
            since_tool_end: Some(25.0),
            ..Default::default()
        };
        assert!(finishing(&s));
        assert!(
            !finishing(&RunSnapshot {
                since_tool_end: Some(12.0),
                ..s.clone()
            }),
            "a pause to think between tools"
        );
        assert!(
            !finishing(&RunSnapshot {
                edits_since_last_test: 1,
                ..s.clone()
            }),
            "edited after the tests"
        );
        assert!(!finishing(&RunSnapshot {
            test_failure_streak: 1,
            last_test_passed: false,
            ..s.clone()
        }));
        let planned = RunSnapshot {
            segments: 4,
            since_tool_end: Some(21.0),
            plan: Some(PlanSnapshot {
                total_steps: 3,
                completed_steps: 3,
            }),
            ..Default::default()
        };
        assert!(finishing(&planned));
    }

    #[test]
    fn single_step_plan_is_not_progress() {
        let s = RunSnapshot {
            plan: Some(PlanSnapshot {
                total_steps: 1,
                completed_steps: 0,
            }),
            ..Default::default()
        };
        assert_eq!(plan_progress(&s), None);
    }
}
