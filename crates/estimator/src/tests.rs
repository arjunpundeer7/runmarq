use super::*;
use crate::eval::{evaluate, EvalRun};
use crate::history::Outcome;
use crate::timeline::{Timeline, TimelineEvent};
use crate::training::{TrainingRun, TrainingSet};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use runmarq_core::event::{
    ActivityCategory, ComplexityBucket, EventType, PlanSnapshot, TaskCategory,
};

fn features(category: TaskCategory) -> PromptFeatures {
    PromptFeatures {
        category,
        complexity: ComplexityBucket::Medium,
        complexity_score: 0.5,
        word_count: 20,
        list_items: 0,
        file_mentions: 0,
        mentions_tests: false,
        fingerprint: None,
        sketch: None,
    }
}

fn snap(elapsed: f64) -> RunSnapshot {
    RunSnapshot {
        active_elapsed: elapsed,
        segment_elapsed: elapsed,
        ..Default::default()
    }
}

fn input<'a>(prompt: Option<&'a PromptFeatures>, snapshot: &'a RunSnapshot) -> ForecastInput<'a> {
    ForecastInput {
        run_id: "run",
        now: Utc::now(),
        agent: AgentSource::ClaudeCode,
        repository_id: Some("repo"),
        prompt,
        effort: None,
        model: None,
        state: RunState::Running,
        snapshot,
        signals: None,
        repo_files: None,
        measured_coverage: None,
    }
}

fn normal(rng: &mut StdRng) -> f64 {
    let u1: f64 = rng.gen_range(1e-12..1.0);
    let u2: f64 = rng.gen_range(0.0..1.0);
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

fn run(i: usize, n: usize, prompt: PromptFeatures, secs: f64) -> TrainingRun {
    TrainingRun {
        id: format!("r{i}"),
        agent: AgentSource::ClaudeCode,
        repository_id: Some("repo".into()),
        prompt: Some(prompt),
        effort: None,
        model: None,
        model_at_start: None,
        outcome: Outcome::Completed,
        active_seconds: secs,
        ended_at: Utc::now() - chrono::Duration::hours((n - i) as i64),
        timeline: None,
        signals: Vec::new(),
        repo_files: None,
    }
}

fn simulated(
    rng: &mut StdRng,
    n: usize,
    median: f64,
    sigma: f64,
    category: TaskCategory,
) -> Vec<TrainingRun> {
    (0..n)
        .map(|i| {
            run(
                i,
                n,
                features(category),
                (median.ln() + sigma * normal(rng)).exp(),
            )
        })
        .collect()
}

fn fitted(runs: Vec<TrainingRun>) -> Estimator {
    Estimator::fit(
        &TrainingSet {
            runs,
            commands: Vec::new(),
        },
        Utc::now(),
    )
}

fn p50(est: &Estimator, i: &ForecastInput<'_>) -> f64 {
    est.forecast(i).remaining_p50_seconds.unwrap() as f64
}

#[test]
fn quantiles_are_ordered_and_nonnegative_everywhere() {
    let mut rng = StdRng::seed_from_u64(7);
    let history = simulated(&mut rng, 50, 240.0, 0.8, TaskCategory::BugFix);
    for est in [Estimator::default(), fitted(history)] {
        for cat in TaskCategory::ALL {
            let p = features(cat);
            for elapsed in [0.0, 1.0, 5.0, 30.0, 120.0, 600.0, 3600.0, 36_000.0, 1e6] {
                let mut s = snap(elapsed);
                s.test_failure_streak = rng.gen_range(0..5);
                s.plan = Some(PlanSnapshot {
                    total_steps: 5,
                    completed_steps: rng.gen_range(0..=5),
                });
                let f = est.forecast(&input(Some(&p), &s));
                assert!(f.is_consistent(), "{f:?}");
                let (a, b, c) = (
                    f.remaining_p10_seconds.unwrap(),
                    f.remaining_p50_seconds.unwrap(),
                    f.remaining_p90_seconds.unwrap(),
                );
                assert!(
                    a >= 1 && a <= b && b <= c,
                    "{cat:?} t={elapsed}: {a} {b} {c}"
                );
                assert!(c as f64 <= MAX_REMAINING_SECONDS);
                let (r10, r90) = (f.raw_p10_seconds.unwrap(), f.raw_p90_seconds.unwrap());
                assert!(r10 <= r90);
            }
        }
    }
}

#[test]
fn conditional_quantiles_are_continuous_at_tail_switch() {
    let (mu, sigma) = (5.0f64, 0.8f64);
    let t_switch = (mu + 8.0 * sigma).exp();
    let below = conditional_remaining(mu, sigma, t_switch * 0.999, 0.5);
    let above = conditional_remaining(mu, sigma, t_switch * 1.001, 0.5);
    assert!((below / above - 1.0).abs() < 0.1, "{below} vs {above}");
}

#[test]
fn remaining_is_conditioned_on_survival_not_subtracted() {
    let est = Estimator::default();
    let p = features(TaskCategory::BugFix);
    let start = p50(&est, &input(Some(&p), &snap(0.0)));
    let late = est.forecast(&input(Some(&p), &snap(start * 3.0)));
    assert!(late.remaining_p50_seconds.unwrap() > 0);
    assert!(late
        .reason_codes
        .iter()
        .any(|r| r == "elapsed_beyond_typical"));
    let early = p50(&est, &input(Some(&p), &snap(start * 0.1)));
    assert!(early < start);
}

#[test]
fn cold_start_is_wide_low_confidence_and_labelled() {
    let p = features(TaskCategory::FeatureImplementation);
    let f = Estimator::default().forecast(&input(Some(&p), &snap(0.0)));
    assert_eq!(f.basis, ForecastBasis::ColdStart);
    assert_eq!(f.confidence, Confidence::Low);
    assert!(f.reason_codes.contains(&"cold_start_prior".to_string()));
    assert!(!f.calibrated);
    assert_eq!(f.components, vec!["history".to_string()]);
    let ratio = f.remaining_p90_seconds.unwrap() as f64 / f.remaining_p10_seconds.unwrap() as f64;
    assert!(ratio > 8.0, "cold-start ranges must be wide, got {ratio}");
}

#[test]
fn missing_prompt_without_history_is_insufficient_data() {
    let s = snap(10.0);
    let f = Estimator::default().forecast(&input(None, &s));
    assert_eq!(f.status, ForecastStatus::InsufficientData);
    assert_eq!(f.remaining_p50_seconds, None);
    assert!(f.is_consistent());
}

#[test]
fn missing_prompt_with_history_uses_global_baseline() {
    let mut rng = StdRng::seed_from_u64(1);
    let est = fitted(simulated(&mut rng, 30, 200.0, 0.7, TaskCategory::BugFix));
    let s = snap(10.0);
    let f = est.forecast(&input(None, &s));
    assert_eq!(f.status, ForecastStatus::Available);
    assert_eq!(
        f.confidence,
        Confidence::Low,
        "unknown task is never better than low confidence"
    );
}

#[test]
fn waiting_and_unknown_states_pause_the_estimate() {
    let p = features(TaskCategory::BugFix);
    let s = snap(100.0);
    let mut i = input(Some(&p), &s);
    i.state = RunState::WaitingForUser;
    let f = Estimator::default().forecast(&i);
    assert_eq!(f.status, ForecastStatus::Waiting);
    assert_eq!(f.remaining_p50_seconds, None);
    i.state = RunState::Unknown;
    assert_eq!(
        Estimator::default().forecast(&i).status,
        ForecastStatus::Unavailable
    );
    i.state = RunState::Completed;
    assert_eq!(
        Estimator::default().forecast(&i).status,
        ForecastStatus::Unavailable
    );
}

#[test]
fn repeated_failures_raise_the_estimate_with_a_reason() {
    let p = features(TaskCategory::BugFix);
    let est = Estimator::default();
    let base = est.forecast(&input(Some(&p), &snap(60.0)));
    let mut s = snap(60.0);
    s.test_runs = 3;
    s.test_failure_streak = 3;
    let worse = est.forecast(&input(Some(&p), &s));
    assert!(worse.remaining_p50_seconds > base.remaining_p50_seconds);
    let smoothed = smooth(Some(&base), worse);
    assert!(smoothed
        .reason_codes
        .contains(&"estimate_increased".to_string()));
    assert_eq!(
        change_message(&smoothed).as_deref(),
        Some("Estimate increased after repeated test failures.")
    );
}

#[test]
fn small_changes_are_smoothed() {
    let p = features(TaskCategory::BugFix);
    let est = Estimator::default();
    let a = est.forecast(&input(Some(&p), &snap(30.0)));
    let b = est.forecast(&input(Some(&p), &snap(45.0)));
    let s = smooth(Some(&a), b);
    assert!(s.reason_codes.contains(&"smoothed".to_string()));
    assert!(s.is_consistent());
    assert_eq!(change_message(&s), None);
}

#[test]
fn outliers_do_not_dominate() {
    let mut rng = StdRng::seed_from_u64(3);
    let mut runs = simulated(&mut rng, 40, 200.0, 0.5, TaskCategory::BugFix);
    let p = features(TaskCategory::BugFix);
    let s = snap(0.0);
    let base = p50(&fitted(runs.clone()), &input(Some(&p), &s));
    let mut outlier = runs[0].clone();
    outlier.id = "outlier".into();
    outlier.active_seconds = 200_000.0;
    runs.insert(0, outlier);
    let with = p50(&fitted(runs), &input(Some(&p), &s));
    let ratio = with / base;
    assert!(
        ratio < 1.25,
        "one extreme outlier moved the median by {ratio}x"
    );
}

#[test]
fn agents_learn_separately() {
    let mut rng = StdRng::seed_from_u64(5);
    let mut runs = simulated(&mut rng, 40, 100.0, 0.4, TaskCategory::BugFix);
    let mut codex = simulated(&mut rng, 40, 900.0, 0.4, TaskCategory::BugFix);
    for (i, r) in codex.iter_mut().enumerate() {
        r.agent = AgentSource::Codex;
        r.id = format!("c{i}");
    }
    runs.extend(codex);
    let est = fitted(runs);
    let p = features(TaskCategory::BugFix);
    let s = snap(0.0);
    let claude = p50(&est, &input(Some(&p), &s));
    let mut ci = input(Some(&p), &s);
    ci.agent = AgentSource::Codex;
    let codex = p50(&est, &ci);
    assert!(codex > claude * 4.0, "claude {claude} codex {codex}");
}

#[test]
fn history_and_calibration_raise_basis_and_confidence() {
    let mut rng = StdRng::seed_from_u64(9);
    let est = fitted(simulated(&mut rng, 80, 300.0, 0.4, TaskCategory::BugFix));
    assert!(est.is_calibrated());
    let p = features(TaskCategory::BugFix);
    let s = snap(0.0);
    let mut i = input(Some(&p), &s);
    let f = est.forecast(&i);
    assert_eq!(f.basis, ForecastBasis::History);
    assert!(f.calibrated);
    assert!(f.reason_codes.contains(&"calibrated".to_string()));
    assert_eq!(f.confidence, Confidence::High, "{f:?}");
    i.measured_coverage = Some(0.5);
    assert_eq!(
        est.forecast(&i).confidence,
        Confidence::Medium,
        "shown forecasts missed too often"
    );
}

#[test]
fn evaluation_withholds_metrics_on_small_samples() {
    let p = features(TaskCategory::BugFix);
    let s = snap(0.0);
    let runs: Vec<EvalRun> = (0..3)
        .map(|i| EvalRun {
            run_id: format!("r{i}"),
            agent: AgentSource::ClaudeCode,
            category: TaskCategory::BugFix,
            outcome: Outcome::Completed,
            active_seconds: 100.0,
            forecasts: vec![Estimator::default().forecast(&input(Some(&p), &s))],
        })
        .collect();
    let r = evaluate(&runs, None);
    assert_eq!(r.evaluated_runs, 3);
    assert_eq!(r.median_abs_error_seconds, None);
    assert_eq!(r.interval_coverage, None);
}

/// Given a world that matches the baseline's family, calibrated 80%
/// intervals should cover about 80% of outcomes, at the start and later.
#[test]
fn calibration_on_simulated_lognormal_world() {
    let mut rng = StdRng::seed_from_u64(42);
    let (median, sigma) = (300.0, 0.6);
    let est = fitted(simulated(
        &mut rng,
        300,
        median,
        sigma,
        TaskCategory::BugFix,
    ));
    assert!(est.is_calibrated());
    let p = features(TaskCategory::BugFix);
    let start = est.forecast(&input(Some(&p), &snap(0.0)));
    let later = est.forecast(&input(Some(&p), &snap(400.0)));

    let (mut start_hits, mut cond_hits, mut cond_n) = (0, 0, 0);
    let n = 3000;
    for _ in 0..n {
        let actual = (f64::ln(median) + sigma * normal(&mut rng)).exp();
        let covered = |f: &EtaForecast, v: f64| {
            (f.remaining_p10_seconds.unwrap() as f64) <= v
                && v <= f.remaining_p90_seconds.unwrap() as f64
        };
        start_hits += usize::from(covered(&start, actual));
        if actual > 400.0 {
            cond_n += 1;
            cond_hits += usize::from(covered(&later, actual - 400.0));
        }
    }
    let start_cov = start_hits as f64 / n as f64;
    let cond_cov = cond_hits as f64 / cond_n as f64;
    assert!(
        (0.72..=0.9).contains(&start_cov),
        "start coverage {start_cov}"
    );
    assert!(
        (0.68..=0.92).contains(&cond_cov),
        "conditional coverage {cond_cov} over {cond_n}"
    );
}

#[test]
fn temporal_holdout_needs_enough_runs() {
    let mut rng = StdRng::seed_from_u64(11);
    assert!(
        fitted(simulated(&mut rng, 10, 200.0, 0.5, TaskCategory::BugFix))
            .holdout()
            .is_none()
    );
    let h = fitted(simulated(&mut rng, 100, 200.0, 0.5, TaskCategory::BugFix))
        .holdout()
        .unwrap();
    assert_eq!(h.train_runs, 80);
    assert_eq!(h.test_runs, 20);
    assert!(h.interval_coverage > 0.5);
}

#[test]
fn cold_status_explains_what_each_model_needs() {
    let s = Estimator::default().status();
    assert_eq!(s.version, MODEL_VERSION);
    assert_eq!(s.finished_runs, 0);
    let active: Vec<bool> = s.components.iter().map(|c| c.active).collect();
    assert_eq!(active, vec![true, false, false, false]);
    assert!(s.components[3].detail.contains("100"));
    assert!(!s.calibration.active);
    assert!(s.calibration.required_runs >= 30);
}

fn tool(
    at: chrono::DateTime<Utc>,
    kind: EventType,
    id: &str,
    a: ActivityCategory,
) -> TimelineEvent {
    TimelineEvent {
        activity: Some(a),
        tool_use_id: Some(id.into()),
        ..TimelineEvent::new(at, kind)
    }
}

/// Read, edit, test; half of the runs fail the first test, edit again and
/// retest. The run ends ten seconds after the last passing test.
fn phased_events(
    start: chrono::DateTime<Utc>,
    fail_first: bool,
    test_secs: i64,
    tail_secs: i64,
) -> (Vec<TimelineEvent>, i64) {
    use ActivityCategory::*;
    use EventType::*;
    let at = |s: i64| start + chrono::Duration::seconds(s);
    let mut ev = vec![
        tool(at(5), ToolStarted, "read", ReadingFiles),
        tool(at(6), ToolCompleted, "read", ReadingFiles),
        tool(at(12), ToolStarted, "edit", EditingCode),
        tool(at(13), ToolCompleted, "edit", EditingCode),
        tool(at(18), ToolStarted, "t1", RunningTests),
    ];
    let mut t = 18 + test_secs;
    if fail_first {
        ev.push(tool(at(t), ToolFailed, "t1", RunningTests));
        ev.push(tool(at(t + 8), ToolStarted, "edit2", EditingCode));
        ev.push(tool(at(t + 9), ToolCompleted, "edit2", EditingCode));
        ev.push(tool(at(t + 14), ToolStarted, "t2", RunningTests));
        t += 14 + test_secs;
        ev.push(tool(at(t), ToolCompleted, "t2", RunningTests));
    } else {
        ev.push(tool(at(t), ToolCompleted, "t1", RunningTests));
    }
    let end = t + tail_secs;
    ev.push(TimelineEvent::new(at(end), RunCompleted));
    (ev, end)
}

fn phased_runs(n: usize, seed: u64) -> Vec<TrainingRun> {
    phased_runs_ending(n, seed, 10)
}

/// Phased runs whose agent writes for `tail_secs` after the last tool.
fn phased_runs_ending(n: usize, seed: u64, tail_secs: i64) -> Vec<TrainingRun> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..n)
        .map(|i| {
            let ended = Utc::now() - chrono::Duration::hours((n - i) as i64);
            let test_secs = rng.gen_range(25..35);
            let fail = i % 2 == 0;
            let end = 18 + tail_secs + test_secs + if fail { 14 + test_secs } else { 0 };
            let start = ended - chrono::Duration::seconds(end);
            let (events, last) = phased_events(start, fail, test_secs, tail_secs);
            assert_eq!(last, end);
            let mut r = run(i, n, features(TaskCategory::BugFix), end as f64);
            r.timeline = Some(Timeline::new(start, events));
            r
        })
        .collect()
}

#[test]
fn phase_model_reads_the_state_of_the_run() {
    let est = fitted(phased_runs(60, 1));
    let status = est.status();
    assert!(status.components[1].active, "{:?}", status.components[1]);

    let start = Utc::now() - chrono::Duration::seconds(60);
    let p = features(TaskCategory::BugFix);
    let forecast_after = |fail: bool| {
        let (events, _) = phased_events(start, fail, 30, 10);
        let at = start + chrono::Duration::seconds(49);
        let tl = Timeline::new(start, events.into_iter().filter(|e| e.at <= at).collect());
        let s = tl.snapshot(at);
        let mut i = input(Some(&p), &s);
        i.now = at;
        est.forecast(&i)
    };
    let passed = forecast_after(false);
    let failed = forecast_after(true);
    assert!(
        passed.components.contains(&"phase".to_string()),
        "{passed:?}"
    );
    let (a, b) = (
        passed.remaining_p50_seconds.unwrap(),
        failed.remaining_p50_seconds.unwrap(),
    );
    assert!(
        b as f64 > a as f64 * 1.5,
        "after a pass {a}s, after a failure {b}s"
    );
}

#[test]
fn similar_prompts_separate_tasks_of_one_category() {
    let salt = b"salt";
    let mut rng = StdRng::seed_from_u64(4);
    let n = 60;
    let runs: Vec<TrainingRun> = (0..n)
        .map(|i| {
            let (text, median) = if i % 2 == 0 {
                (
                    format!(
                        "migrate the billing database schema to the new ledger tables part {i}"
                    ),
                    1500.0,
                )
            } else {
                (
                    format!("fix the typo in the readme installation heading {i}"),
                    40.0,
                )
            };
            let mut f = features(TaskCategory::BugFix);
            f.sketch = Some(classify::sketch(&text, salt));
            run(i, n, f, (f64::ln(median) + 0.3 * normal(&mut rng)).exp())
        })
        .collect();
    let est = fitted(runs);
    let ask = |text: &str| {
        let mut f = features(TaskCategory::BugFix);
        f.sketch = Some(classify::sketch(text, salt));
        let s = snap(0.0);
        est.forecast(&input(Some(&f), &s))
    };
    let big = ask("migrate the billing database schema to ledger tables");
    let small = ask("fix typo in the readme heading");
    assert!(big.components.contains(&"similar".to_string()), "{big:?}");
    let (b, s) = (
        big.remaining_p50_seconds.unwrap(),
        small.remaining_p50_seconds.unwrap(),
    );
    assert!(b as f64 > s as f64 * 5.0, "migration {b}s, typo {s}s");
}

#[test]
fn learned_model_waits_for_enough_runs_then_finds_structure() {
    let world = |n: usize, seed: u64| -> Vec<TrainingRun> {
        let mut rng = StdRng::seed_from_u64(seed);
        (0..n)
            .map(|i| {
                let mut f = features(TaskCategory::BugFix);
                f.list_items = (i % 6) as u32;
                let median = 40.0 * (1.0 + 2.0 * f.list_items as f64);
                run(i, n, f, (median.ln() + 0.25 * normal(&mut rng)).exp())
            })
            .collect()
    };
    let small = fitted(world(60, 1));
    assert!(!small.status().components[3].active);

    let est = fitted(world(160, 2));
    let status = est.status();
    assert!(status.components[3].active, "{:?}", status.components[3]);
    let s = snap(0.0);
    let mut short = features(TaskCategory::BugFix);
    short.list_items = 0;
    let mut long = short.clone();
    long.list_items = 5;
    let a = p50(&est, &input(Some(&short), &s));
    let b = p50(&est, &input(Some(&long), &s));
    assert!(b > a * 3.0, "0 items {a}s, 5 items {b}s");
    assert!(est
        .forecast(&input(Some(&long), &s))
        .components
        .contains(&"learned".to_string()));
}

#[test]
fn finishing_signs_switch_to_what_followed_them_before() {
    let est = fitted(phased_runs_ending(60, 2, 40));
    assert!(
        est.models.finishing_runs >= 4,
        "{}",
        est.models.finishing_runs
    );
    assert!(est.models.finishing[1] < 30.0, "{:?}", est.models.finishing);
    assert!(est.models.finishing_confirmed());

    let start = Utc::now() - chrono::Duration::seconds(120);
    let p = features(TaskCategory::BugFix);
    let at_second = |s: i64| {
        let (events, _) = phased_events(start, false, 30, 40);
        let at = start + chrono::Duration::seconds(s);
        let tl = Timeline::new(start, events.into_iter().filter(|e| e.at <= at).collect());
        let snap = tl.snapshot(at);
        let mut i = input(Some(&p), &snap);
        i.now = at;
        est.forecast(&i)
    };
    // The test passes at 48 s; the run is quiet from then on.
    let writing = at_second(72);
    assert!(
        writing.reason_codes.iter().any(|r| r == "finishing_up"),
        "{writing:?}"
    );
    assert_eq!(
        writing.outlook(),
        Some(runmarq_core::forecast::Outlook::FinishingUp)
    );
    let just_passed = at_second(58);
    assert!(!just_passed.reason_codes.iter().any(|r| r == "finishing_up"));
}

#[test]
fn plan_pace_shapes_the_final_forecast() {
    let mut rng = StdRng::seed_from_u64(4);
    let est = fitted(simulated(
        &mut rng,
        40,
        1800.0,
        0.6,
        TaskCategory::FeatureImplementation,
    ));
    let p = features(TaskCategory::FeatureImplementation);
    let mut s = snap(300.0);
    let without = p50(&est, &input(Some(&p), &s));
    s.plan = Some(PlanSnapshot {
        total_steps: 4,
        completed_steps: 3,
    });
    let f = est.forecast(&input(Some(&p), &s));
    assert!(f.reason_codes.iter().any(|r| r == "plan_progress"));
    let with = f.remaining_p50_seconds.unwrap() as f64;
    assert!(
        with < without * 0.6,
        "plan says ~100 s left: {with} vs {without}"
    );
}

#[test]
fn a_slow_run_is_simulated_slower() {
    let est = fitted(phased_runs(60, 3));
    let phase = est.models.phase.as_ref().expect("phase model");
    let typical = phase.pace(&RunSnapshot {
        active_elapsed: 60.0,
        segments: 4,
        ..Default::default()
    });
    let slow = phase.pace(&RunSnapshot {
        active_elapsed: 600.0,
        segments: 4,
        ..Default::default()
    });
    assert!(slow > typical * 1.5, "{slow} vs {typical}");
    assert!(slow <= 3.0 + 1e-9, "capped");
    assert_eq!(
        phase.pace(&RunSnapshot::default()),
        1.0,
        "too young to judge"
    );
}

#[test]
fn a_runs_own_model_is_only_known_once_its_transcript_was_read() {
    let mut r = run(0, 1, features(TaskCategory::BugFix), 600.0);
    r.model = Some("claude-haiku-4-5".into());
    r.model_at_start = Some("claude-opus-5-5".into());
    assert_eq!(r.model_at(0.0), Some("claude-opus-5-5"));
    r.signals = vec![(30.0, runmarq_core::signals::RunSignals::default())];
    assert_eq!(r.model_at(10.0), Some("claude-opus-5-5"));
    assert_eq!(r.model_at(45.0), Some("claude-haiku-4-5"));
    r.model_at_start = None;
    assert_eq!(
        r.model_at(0.0),
        None,
        "a quick run's model must not leak into its start"
    );
}

#[test]
fn disagreeing_components_are_pooled_not_averaged() {
    let quick = [10f64.ln(), 20f64.ln(), 40f64.ln()];
    let long = [600f64.ln(), 1200f64.ln(), 2400f64.ln()];
    let (q, used) = training::mixture(
        &[Some(quick), Some(long), None, None],
        &[1.0, 1.0, 0.0, 0.0],
    )
    .unwrap();
    assert!(used[0] && used[1] && !used[2]);
    let [p10, p50, p90] = q.map(f64::exp);
    assert!(p10 < 25.0, "the quick answer stays possible: {p10}");
    assert!(p90 > 1000.0, "and so does the long task: {p90}");
    assert!(p10 < p50 && p50 < p90);
    let (same, _) =
        training::mixture(&[Some(long), Some(long), None, None], &[1.0, 3.0, 0.0, 0.0]).unwrap();
    for (a, b) in same.iter().zip(long.iter()) {
        assert!(
            (a - b).abs() < 0.01,
            "agreeing components pass through unchanged"
        );
    }
}

#[test]
fn a_change_is_explained_only_by_evidence_pointing_the_same_way() {
    let p = features(TaskCategory::BugFix);
    let mut f = Estimator::default().forecast(&input(Some(&p), &snap(60.0)));
    f.reason_codes = vec!["test_failure".into(), "estimate_decreased".into()];
    assert_eq!(
        change_message(&f).as_deref(),
        Some("Estimate decreased as the run progressed."),
        "a failure never explains a shorter estimate"
    );
    f.reason_codes = vec![
        "test_failure".into(),
        "tests_passing".into(),
        "estimate_decreased".into(),
    ];
    assert_eq!(
        change_message(&f).as_deref(),
        Some("Estimate decreased after tests passed.")
    );
    f.reason_codes = vec!["tests_passing".into(), "estimate_increased".into()];
    assert_eq!(
        change_message(&f).as_deref(),
        Some("Estimate increased as the run progressed.")
    );
}
