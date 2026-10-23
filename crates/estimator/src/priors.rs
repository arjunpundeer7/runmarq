//! Cold-start priors.
//!
//! These are deliberately conservative, wide starting points, not measurements.
//! Medians are typical *active* durations of one agent turn for each kind of
//! request; sigma is the log-space spread. A sigma of 1.0 makes the nominal
//! P10–P90 band span roughly 0.28x to 3.6x the median, which is honest about
//! how little is known before any local history exists.
//!
//! Every forecast built mostly from these values carries the reason code
//! `cold_start_prior` and `Low` confidence.

use runmarq_core::event::{ComplexityBucket, TaskCategory};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prior {
    pub median_seconds: f64,
    pub sigma: f64,
}

pub fn category_prior(category: TaskCategory) -> Prior {
    let (median_seconds, sigma) = match category {
        TaskCategory::SmallEdit => (45.0, 0.9),
        TaskCategory::BugFix => (180.0, 1.0),
        TaskCategory::FeatureImplementation => (420.0, 1.0),
        TaskCategory::UiDevelopment => (300.0, 1.0),
        TaskCategory::Refactoring => (300.0, 1.0),
        TaskCategory::Testing => (240.0, 1.0),
        TaskCategory::DependencyWork => (180.0, 1.0),
        TaskCategory::RepositoryAnalysis => (90.0, 0.9),
        TaskCategory::Research => (120.0, 1.0),
        TaskCategory::Migration => (600.0, 1.1),
        TaskCategory::MultiStep => (600.0, 1.1),
        TaskCategory::Unknown => (150.0, 1.2),
    };
    Prior {
        median_seconds,
        sigma,
    }
}

pub fn complexity_multiplier(c: Option<ComplexityBucket>) -> f64 {
    match c {
        Some(ComplexityBucket::Trivial) => 0.5,
        Some(ComplexityBucket::Small) => 0.75,
        Some(ComplexityBucket::Medium) | None => 1.0,
        Some(ComplexityBucket::Large) => 1.6,
        Some(ComplexityBucket::VeryLarge) => 2.5,
    }
}

/// Log of the prior median for a task, the baseline that historical
/// residuals are measured against.
pub fn log_baseline(category: TaskCategory, complexity: Option<ComplexityBucket>) -> f64 {
    (category_prior(category).median_seconds * complexity_multiplier(complexity)).ln()
}
