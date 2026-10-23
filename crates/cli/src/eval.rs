//! `runmarq eval`: how the estimates shown would have held up on the runs
//! recorded on this computer.

use crate::ui::{self, bold, dim};
use crate::Output;
use anyhow::{Context, Result};
use runmarq_collector::{engine::HISTORY_LIMIT, learning};
use runmarq_core::paths::Paths;
use runmarq_estimator::backtest::{self, Report, Tally};
use runmarq_storage::Store;
use std::process::ExitCode;

/// Below this many evaluated runs the figures are shown with a warning.
const FEW_RUNS: usize = 10;

pub fn eval(paths: &Paths, out: Output) -> Result<ExitCode> {
    let db = paths.database();
    if !db.is_file() {
        anyhow::bail!("no run history yet at {}", db.display());
    }
    let store = Store::open(&db).with_context(|| format!("could not open {}", db.display()))?;
    let set = learning::training_set(&store, HISTORY_LIMIT)?;
    let report = backtest::run(&set);
    if out.json {
        ui::print_json(&report)?;
    } else {
        print(&report);
    }
    Ok(ExitCode::SUCCESS)
}

fn pct(v: Option<f64>) -> String {
    v.map(|x| format!("{:.0}%", x * 100.0))
        .unwrap_or_else(|| "—".into())
}

fn factor(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.1}x")).unwrap_or_else(|| "—".into())
}

fn row(label: &str, t: &Tally) {
    println!(
        "  {:<14} {:>9} {:>12} {:>12} {:>9} {:>9} {:>8}",
        label,
        t.forecasts,
        pct(t.likely_rate()),
        pct(t.range_rate()),
        pct(t.outlook_rate()),
        factor(t.error_factor()),
        factor(t.width_factor()),
    );
}

fn print(r: &Report) {
    println!(
        "{}",
        bold("How the estimates would have held up on your runs")
    );
    println!(
        "{}",
        dim("Each run is forecast by models fitted only on runs that ended before it.")
    );
    println!();
    println!(
        "  {} finished runs, {} evaluated",
        r.finished_runs, r.evaluated_runs
    );
    if r.evaluated_runs == 0 {
        println!("  Not enough runs yet: at least six must have finished.");
        return;
    }
    println!();
    println!(
        "  {:<14} {:>9} {:>12} {:>12} {:>9} {:>9} {:>8}",
        "", "forecasts", "likely range", "full range", "outlook", "error", "width"
    );
    for m in r.by_moment.iter().filter(|m| m.tally.forecasts > 0) {
        row(m.label, &m.tally);
    }
    row("All", &r.overall);
    let o = &r.overall;
    println!();
    println!(
        "  Outside the full range: {} ended sooner, {} ended later.",
        o.sooner_than_range, o.later_than_range
    );
    println!();
    println!(
        "{}",
        dim("  likely range: inside P25–P75 (ideal 50%) · full range: inside P10–P90 (ideal 80%)")
    );
    println!(
        "{}",
        dim("  outlook: the headline held · error: typical factor between the median and the actual time")
    );
    println!(
        "{}",
        dim("  width: typical ratio of the likely range's upper end to its lower end")
    );
    if r.finishing_forecasts > 0 {
        println!();
        let shown = match r.finishing_forecasts {
            1 => "once".to_string(),
            n => format!("{n} times"),
        };
        println!(
            "  \u{201c}Finishing up\u{201d} was shown {shown}; {} ended within 3 min.",
            r.finishing_within_3_min
        );
    }
    if r.evaluated_runs < FEW_RUNS {
        println!();
        println!(
            "  {}",
            ui::yellow("Few runs so far, so these figures can swing a lot.")
        );
    }
}
