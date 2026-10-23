//! Times model fitting and forecasting on a synthetic history.
//!
//! `cargo run --release -p runmarq-estimator --example fit_timing -- 3000`

use chrono::{Duration, Utc};
use runmarq_core::event::{
    ActivityCategory, AgentSource, ComplexityBucket, EventType, PromptFeatures, TaskCategory,
};
use runmarq_core::run::RunState;
use runmarq_estimator::classify::sketch;
use runmarq_estimator::history::Outcome;
use runmarq_estimator::rng::Rng;
use runmarq_estimator::timeline::{Timeline, TimelineEvent};
use runmarq_estimator::training::{TrainingRun, TrainingSet};
use runmarq_estimator::{Estimator, ForecastInput};
use std::time::Instant;

const TASKS: [&str; 6] = [
    "fix the failing login test",
    "add pagination to the orders table",
    "refactor the billing service client",
    "update dependencies and fix the build",
    "write tests for the csv importer",
    "investigate slow startup of the api server",
];

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3000);
    let mut rng = Rng::new(1);
    let now = Utc::now();
    let runs: Vec<TrainingRun> = (0..n)
        .map(|i| {
            let task = i % TASKS.len();
            let steps = 3 + rng.below(25);
            let start = now - Duration::minutes(((n - i) * 30) as i64);
            let mut t = 0i64;
            let mut events = vec![TimelineEvent::new(start, EventType::PromptSubmitted)];
            for s in 0..steps {
                let activity = [
                    ActivityCategory::ReadingFiles,
                    ActivityCategory::EditingCode,
                    ActivityCategory::RunningTests,
                    ActivityCategory::SearchingRepository,
                ][rng.below(4)];
                t += 2 + rng.below(20) as i64;
                let id = format!("t{s}");
                let mk = |at: i64, kind| TimelineEvent {
                    activity: Some(activity),
                    tool_use_id: Some(id.clone()),
                    ..TimelineEvent::new(start + Duration::seconds(at), kind)
                };
                events.push(mk(t, EventType::ToolStarted));
                t += if activity == ActivityCategory::RunningTests {
                    20 + rng.below(60) as i64
                } else {
                    1
                };
                let kind = if rng.uniform() < 0.1 {
                    EventType::ToolFailed
                } else {
                    EventType::ToolCompleted
                };
                events.push(mk(t, kind));
            }
            t += 5;
            events.push(TimelineEvent::new(
                start + Duration::seconds(t),
                EventType::RunCompleted,
            ));
            TrainingRun {
                id: format!("r{i}"),
                agent: AgentSource::ALL[i % 3],
                repository_id: Some(format!("repo{}", i % 7)),
                prompt: Some(PromptFeatures {
                    category: TaskCategory::ALL[task],
                    complexity: ComplexityBucket::Medium,
                    complexity_score: 0.5,
                    word_count: 12,
                    list_items: 0,
                    file_mentions: 1,
                    mentions_tests: task == 4,
                    fingerprint: None,
                    sketch: Some(sketch(TASKS[task], b"bench")),
                }),
                effort: Some(["low", "medium", "high"][i % 3].into()),
                model: Some("model-a".into()),
                model_at_start: Some("model-a".into()),
                outcome: Outcome::Completed,
                active_seconds: t as f64,
                ended_at: start + Duration::seconds(t),
                timeline: Some(Timeline::new(start, events)),
                signals: Vec::new(),
                repo_files: Some(1200),
            }
        })
        .collect();

    let set = TrainingSet {
        runs,
        commands: Vec::new(),
    };
    let started = Instant::now();
    let est = Estimator::fit(&set, now);
    let fit = started.elapsed();
    let r = est.report();
    println!(
        "fit {n} runs in {:.2}s; validation {} runs / {} samples; weights {:?}; calibrated {}",
        fit.as_secs_f64(),
        r.validation_runs,
        r.validation_samples,
        est.weights(),
        est.is_calibrated()
    );

    let run = &set.runs[n - 1];
    let snap = run.snapshot_at(run.active_seconds / 2.0);
    let input = ForecastInput {
        run_id: "live",
        now,
        agent: run.agent,
        repository_id: run.repository_id.as_deref(),
        prompt: run.prompt.as_ref(),
        effort: run.effort.as_deref(),
        model: run.model.as_deref(),
        state: RunState::Running,
        snapshot: &snap,
        signals: None,
        repo_files: run.repo_files,
        measured_coverage: None,
    };
    let started = Instant::now();
    let rounds = 200;
    let mut last = None;
    for _ in 0..rounds {
        last = Some(est.forecast(&input));
    }
    let per = started.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
    let f = last.expect("ran");
    println!(
        "forecast {per:.2} ms; p10/p50/p90 {:?}/{:?}/{:?}; components {:?}",
        f.remaining_p10_seconds, f.remaining_p50_seconds, f.remaining_p90_seconds, f.components
    );
}
