//! Forecast contract.
//!
//! P10/P50/P90 are predicted quantiles of *remaining* time. The P10–P90 span
//! is a nominal 80% interval; whether it actually covers 80% of outcomes is
//! measured by the evaluation module, never assumed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Confidence::Low),
            "medium" => Some(Confidence::Medium),
            "high" => Some(Confidence::High),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ForecastStatus {
    Estimating,
    Available,
    Waiting,
    InsufficientData,
    Unavailable,
}

impl ForecastStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ForecastStatus::Estimating => "estimating",
            ForecastStatus::Available => "available",
            ForecastStatus::Waiting => "waiting",
            ForecastStatus::InsufficientData => "insufficient_data",
            ForecastStatus::Unavailable => "unavailable",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

/// Where the duration distribution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ForecastBasis {
    /// Built-in conservative priors only; no local history yet.
    ColdStart,
    /// Priors shrunk toward local history.
    Blended,
    /// Dominated by local historical runs.
    History,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EtaForecast {
    pub run_id: String,
    #[ts(type = "string")]
    pub generated_at: DateTime<Utc>,
    pub remaining_p10_seconds: Option<u32>,
    pub remaining_p50_seconds: Option<u32>,
    pub remaining_p90_seconds: Option<u32>,
    #[ts(type = "string | null")]
    pub expected_finish_at: Option<DateTime<Utc>>,
    pub confidence: Confidence,
    pub status: ForecastStatus,
    pub reason_codes: Vec<String>,
    pub model_version: String,
    pub basis: ForecastBasis,
    /// Effective number of historical runs behind the estimate.
    pub sample_size: f32,
    /// Active (non-waiting) seconds elapsed when the forecast was produced.
    pub active_elapsed_seconds: u32,
    /// Quantiles before calibration, kept so calibration can be measured.
    #[serde(default)]
    pub raw_p10_seconds: Option<u32>,
    #[serde(default)]
    pub raw_p50_seconds: Option<u32>,
    #[serde(default)]
    pub raw_p90_seconds: Option<u32>,
    /// Whether conformal calibration adjusted the interval.
    #[serde(default)]
    pub calibrated: bool,
    /// Models that contributed, for example `history`, `phase`, `learned`.
    #[serde(default)]
    pub components: Vec<String>,
}

impl EtaForecast {
    /// Quantiles must be ordered and nonnegative whenever present.
    pub fn is_consistent(&self) -> bool {
        match (
            self.remaining_p10_seconds,
            self.remaining_p50_seconds,
            self.remaining_p90_seconds,
        ) {
            (Some(a), Some(b), Some(c)) => a <= b && b <= c,
            (None, None, None) => true,
            (None, None, Some(_)) => true,
            _ => false,
        }
    }

    pub fn outlook(&self) -> Option<Outlook> {
        let (a, m, b) = (
            self.remaining_p10_seconds?,
            self.remaining_p50_seconds?,
            self.remaining_p90_seconds?,
        );
        Some(if self.reason_codes.iter().any(|r| r == FINISHING_UP) {
            Outlook::FinishingUp
        } else {
            Outlook::of(a, m, b)
        })
    }

    /// What to show when all three quantiles are present. With low
    /// confidence the headline is the outlook and the likely range sits
    /// under it; with medium or high confidence the range leads.
    pub fn words(&self) -> Option<EtaWords> {
        let outlook = self.outlook()?;
        let (likely, tail) = humanize_eta(
            self.remaining_p10_seconds?,
            self.remaining_p50_seconds?,
            self.remaining_p90_seconds?,
        );
        let ranged = self.confidence != Confidence::Low
            && !matches!(outlook, Outlook::FinishingUp | Outlook::AlmostDone);
        let (headline, detail) = if ranged {
            (likely, outlook.label().to_string())
        } else {
            (outlook.label().to_string(), likely)
        };
        Some(EtaWords {
            headline,
            detail,
            tail,
            outlook,
        })
    }
}

/// One line for menus and terminals: "Time for something else, about
/// 12–25 min" or "About 6–9 min remaining".
pub fn eta_line(w: &EtaWords) -> String {
    if w.headline == w.outlook.label() {
        let mut d = w.detail.clone();
        if let Some(first) = d.get(..1) {
            d.replace_range(..1, &first.to_lowercase());
        }
        format!("{}, {d}", w.headline)
    } else {
        format!("{} remaining", w.headline)
    }
}

/// Reason code set when the run shows signs of wrapping up.
pub const FINISHING_UP: &str = "finishing_up";

/// What a forecast means for someone deciding whether to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outlook {
    /// The plan is done, or tests pass after the last edit, and the agent
    /// has stopped starting tools.
    FinishingUp,
    /// Likely done within 2 minutes.
    AlmostDone,
    /// Likely done within 10 minutes.
    FewMinutes,
    /// Most likely done within half an hour.
    AWhile,
    LongTask,
}

impl Outlook {
    pub fn of(p10: u32, p50: u32, p90: u32) -> Outlook {
        let (_, hi) = likely_range(p10, p50, p90);
        if hi <= 120 {
            Outlook::AlmostDone
        } else if hi <= 600 {
            Outlook::FewMinutes
        } else if p50 <= 1800 {
            Outlook::AWhile
        } else {
            Outlook::LongTask
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outlook::FinishingUp => "Finishing up",
            Outlook::AlmostDone => "Almost done",
            Outlook::FewMinutes => "A few minutes",
            Outlook::AWhile => "Time for something else",
            Outlook::LongTask => "Long task",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtaWords {
    pub headline: String,
    pub detail: String,
    /// "could take up to …", when the tail reaches well past the likely range.
    pub tail: Option<String>,
    pub outlook: Outlook,
}

/// How far the quartiles sit from the median relative to the 10th and 90th
/// percentiles, on a normal scale: 0.674 / 1.282.
const QUARTILE_RATIO: f64 = 0.674_49 / 1.281_55;

/// The likely range: the 25th to 75th percentile, read from the 10th, 50th
/// and 90th on a log scale. Each side keeps its own spread, so a long tail
/// above the median does not widen the range below it.
pub fn likely_range(p10: u32, p50: u32, p90: u32) -> (u32, u32) {
    let ln = |v: u32| f64::from(v.max(1)).ln();
    let m = ln(p50);
    let lo = (m - QUARTILE_RATIO * (m - ln(p10.min(p50)))).exp();
    let hi = (m + QUARTILE_RATIO * (ln(p90.max(p50)) - m)).exp();
    (lo.round() as u32, hi.round() as u32)
}

/// The likely range as text, plus "could take up to …" from the 90th
/// percentile when the tail reaches at least half again past the range.
pub fn humanize_eta(p10: u32, p50: u32, p90: u32) -> (String, Option<String>) {
    let (lo, hi) = likely_range(p10, p50, p90);
    let far = f64::from(p90) >= 1.5 * f64::from(hi.max(30));
    let tail = (p90 >= 120 && far && unit_round(p90) != unit_round(hi.max(30))).then(|| {
        let (v, u) = unit_round(p90);
        format!("could take up to {v} {u}")
    });
    (humanize_range(lo, hi), tail)
}

fn unit_round(s: u32) -> (u32, &'static str) {
    if s < 90 {
        (s.max(1), "s")
    } else if s < 3600 * 2 {
        (((s as f64) / 60.0).round().max(1.0) as u32, "min")
    } else {
        (((s as f64) / 3600.0).round() as u32, "h")
    }
}

/// Human-friendly ranges that avoid false precision.
pub fn humanize_range(p10: u32, p90: u32) -> String {
    if p90 < 60 {
        return "Likely under a minute".to_string();
    }
    if p90 < 120 {
        return "Likely under 2 min".to_string();
    }
    let (lo, lu) = unit_round(p10.max(30));
    let (hi, hu) = unit_round(p90);
    if lu == hu {
        if lo >= hi {
            format!("About {hi} {hu}")
        } else {
            format!("About {lo}–{hi} {hu}")
        }
    } else {
        format!("About {lo} {lu} – {hi} {hu}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize() {
        assert_eq!(humanize_range(5, 40), "Likely under a minute");
        assert_eq!(humanize_range(30, 100), "Likely under 2 min");
        assert_eq!(humanize_range(300, 720), "About 5–12 min");
        assert_eq!(humanize_range(1800, 9000), "About 30 min – 3 h");
    }

    #[test]
    fn likely_range_is_the_central_half() {
        // A symmetric log-normal: quartiles at exp(±0.674σ) around the median.
        let s = 1.0f64;
        let q = |z: f64| (600.0 * (z * s).exp()).round() as u32;
        let (lo, hi) = likely_range(q(-1.28155), 600, q(1.28155));
        assert!((i64::from(lo) - i64::from(q(-0.67449))).abs() <= 1, "{lo}");
        assert!((i64::from(hi) - i64::from(q(0.67449))).abs() <= 1, "{hi}");
        // A long upper tail does not drag the lower end down.
        assert_eq!(
            likely_range(300, 600, 7200).0,
            likely_range(300, 600, 900).0
        );
    }

    #[test]
    fn eta_text_names_the_tail_only_when_it_matters() {
        let (text, tail) = humanize_eta(57, 400, 2700);
        assert_eq!(text, "About 2–18 min");
        assert_eq!(tail.as_deref(), Some("could take up to 45 min"));
        let (_, tail) = humanize_eta(300, 420, 560);
        assert_eq!(tail, None);
    }

    fn available(q: [u32; 3], confidence: Confidence, reasons: &[&str]) -> EtaForecast {
        EtaForecast {
            run_id: "r".into(),
            generated_at: Utc::now(),
            remaining_p10_seconds: Some(q[0]),
            remaining_p50_seconds: Some(q[1]),
            remaining_p90_seconds: Some(q[2]),
            expected_finish_at: None,
            confidence,
            status: ForecastStatus::Available,
            reason_codes: reasons.iter().map(|r| r.to_string()).collect(),
            model_version: "x".into(),
            basis: ForecastBasis::Blended,
            sample_size: 0.0,
            active_elapsed_seconds: 0,
            raw_p10_seconds: None,
            raw_p50_seconds: None,
            raw_p90_seconds: None,
            calibrated: false,
            components: vec![],
        }
    }

    #[test]
    fn outlook_answers_wait_or_switch() {
        assert_eq!(Outlook::of(10, 40, 90), Outlook::AlmostDone);
        assert_eq!(Outlook::of(60, 180, 600), Outlook::FewMinutes);
        assert_eq!(Outlook::of(150, 700, 2000), Outlook::AWhile);
        assert_eq!(Outlook::of(900, 2400, 6000), Outlook::LongTask);
    }

    #[test]
    fn low_confidence_leads_with_the_outlook() {
        let w = available([150, 700, 2000], Confidence::Low, &[])
            .words()
            .unwrap();
        assert_eq!(w.headline, "Time for something else");
        assert_eq!(w.detail, "About 5–20 min");
        assert_eq!(eta_line(&w), "Time for something else, about 5–20 min");
        let w = available([150, 700, 2000], Confidence::Medium, &[])
            .words()
            .unwrap();
        assert_eq!(w.headline, "About 5–20 min");
        assert_eq!(w.detail, "Time for something else");
        assert_eq!(eta_line(&w), "About 5–20 min remaining");
        let w = available([20, 60, 200], Confidence::High, &[FINISHING_UP])
            .words()
            .unwrap();
        assert_eq!(w.outlook, Outlook::FinishingUp);
        assert_eq!(w.headline, "Finishing up");
    }

    #[test]
    fn serializes_camel_case() {
        let f = EtaForecast {
            run_id: "r".into(),
            generated_at: Utc::now(),
            remaining_p10_seconds: Some(1),
            remaining_p50_seconds: Some(2),
            remaining_p90_seconds: Some(3),
            expected_finish_at: None,
            confidence: Confidence::Low,
            status: ForecastStatus::Available,
            reason_codes: vec![],
            model_version: "x".into(),
            basis: ForecastBasis::ColdStart,
            sample_size: 0.0,
            active_elapsed_seconds: 0,
            raw_p10_seconds: None,
            raw_p50_seconds: None,
            raw_p90_seconds: None,
            calibrated: false,
            components: vec![],
        };
        let v = serde_json::to_value(&f).unwrap();
        assert!(v.get("remainingP50Seconds").is_some());
        assert!(f.is_consistent());
    }
}
