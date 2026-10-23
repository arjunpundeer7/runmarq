//! `runmarq status`, `doctor`, `history`, `export`, `forget`.

use crate::collector_cmd::running_client;
use crate::integrations::{state_label, statuses};
use crate::ui::{self, bold, dim, mark};
use crate::Output;
use anyhow::{Context, Result};
use runmarq_adapters::Scope;
use runmarq_core::api::{Diagnostics, HistoryPage, IntegrationState, RunView};
use runmarq_core::paths::Paths;
use std::process::ExitCode;
use std::time::Instant;

fn agent_line(r: &RunView) -> String {
    let what = r
        .title
        .clone()
        .unwrap_or_else(|| r.task_category.label().to_string());
    let repo = r
        .repository_name
        .clone()
        .map(|n| format!(" · {n}"))
        .unwrap_or_default();
    format!("{}{}  {}", bold(r.agent.display_name()), dim(&repo), what)
}

pub fn status(paths: &Paths, out: Output) -> Result<ExitCode> {
    let Some(c) = running_client(paths) else {
        if out.json {
            ui::print_json(&serde_json::json!({ "collector": "not_running", "runs": [] }))?;
        } else {
            println!(
                "{} The collector is not running. Start it with `runmarq collector start`.",
                mark(false)
            );
        }
        return Ok(ExitCode::from(3));
    };
    let runs: Vec<RunView> = c.get("/v1/runs/active")?;
    if out.json {
        ui::print_json(&serde_json::json!({ "collector": "running", "runs": runs }))?;
        return Ok(ExitCode::SUCCESS);
    }
    if runs.is_empty() {
        println!("No active runs.");
    } else {
        let n = runs.iter().filter(|r| r.state.is_working()).count();
        println!(
            "{}\n",
            bold(&format!(
                "{} {} working",
                n,
                if n == 1 { "agent" } else { "agents" }
            ))
        );
        for r in &runs {
            println!("{}", agent_line(r));
            let activity = r.current_activity.map(|a| a.label()).unwrap_or("Working");
            println!(
                "  {}  {} elapsed  {}",
                ui::eta(r),
                ui::duration(r.wall_ms),
                dim(activity)
            );
            if let Some(f) = &r.forecast {
                if !f.reason_codes.is_empty() {
                    println!(
                        "  {}",
                        dim(&format!(
                            "confidence {} · {}",
                            f.confidence.as_str(),
                            f.reason_codes.join(", ")
                        ))
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn history(paths: &Paths, limit: u32, out: Output) -> Result<ExitCode> {
    let c = running_client(paths)
        .context("the collector is not running; start it with `runmarq collector start`")?;
    let page: HistoryPage = c.get(&format!("/v1/runs?limit={}", limit.min(500)))?;
    if out.json {
        ui::print_json(&page)?;
        return Ok(ExitCode::SUCCESS);
    }
    for r in &page.runs {
        let est = match (
            r.initial_p10_seconds,
            r.initial_p50_seconds,
            r.initial_p90_seconds,
        ) {
            (Some(a), Some(m), Some(b)) => runmarq_core::forecast::humanize_eta(a, m, b).0,
            _ => "no estimate".into(),
        };
        println!(
            "{}  {:<16} {:>8}  {}  {}",
            r.started_at
                .with_timezone(&chrono::Local)
                .format("%b %d %H:%M"),
            r.state.label(),
            ui::duration(r.wall_ms),
            agent_line(r),
            dim(&format!("estimated {est}"))
        );
    }
    println!(
        "{}",
        dim(&format!("{} of {} runs", page.runs.len(), page.total))
    );
    Ok(ExitCode::SUCCESS)
}

pub fn export(paths: &Paths, output: Option<std::path::PathBuf>) -> Result<ExitCode> {
    let c = running_client(paths)
        .context("the collector is not running; start it with `runmarq collector start`")?;
    let csv = c.get_text("/v1/export.csv")?;
    match output {
        Some(p) => {
            std::fs::write(&p, csv)?;
            eprintln!("{} Wrote {}", mark(true), p.display());
        }
        None => print!("{csv}"),
    }
    Ok(ExitCode::SUCCESS)
}

pub fn forget(paths: &Paths, before: Option<String>, yes: bool) -> Result<ExitCode> {
    let c = running_client(paths)
        .context("the collector is not running; start it with `runmarq collector start`")?;
    let (query, what) = match &before {
        Some(d) => {
            let date = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .context("--before expects YYYY-MM-DD")?;
            let ts = date.and_hms_opt(0, 0, 0).expect("valid").and_utc();
            (
                format!("?before={}", ts.to_rfc3339().replace('+', "%2B")),
                format!("runs that started before {d}"),
            )
        }
        None => (String::new(), "all finished run history".into()),
    };
    if !yes && !ui::confirm(&format!("Delete {what}? This cannot be undone.")) {
        eprintln!("Nothing deleted.");
        return Ok(ExitCode::from(2));
    }
    let r: serde_json::Value = c.delete(&format!("/v1/history{query}"))?;
    println!("{} Deleted {} runs.", mark(true), r["deleted"]);
    Ok(ExitCode::SUCCESS)
}

pub fn doctor(paths: &Paths, out: Output) -> Result<ExitCode> {
    let mut problems = Vec::new();
    let client = running_client(paths);
    let health_ms = client.as_ref().map(|c| {
        let t = Instant::now();
        let _ = c.health();
        t.elapsed().as_secs_f64() * 1000.0
    });
    let diag: Option<Diagnostics> = client.as_ref().and_then(|c| c.get("/v1/diagnostics").ok());
    let integrations = statuses(paths, &Scope::User);
    let (spool_files, spool_bytes) = runmarq_collector::spool::pending(paths);
    let hook_bin = runmarq_adapters::binary::installed_path(&paths.root);

    if client.is_none() {
        problems.push("The collector is not running. Events are spooled until it starts: `runmarq collector start`.".to_string());
    }
    for s in &integrations {
        if s.state == IntegrationState::Error {
            problems.push(format!(
                "{} integration needs attention: run `runmarq integrations verify {}`.",
                s.display_name,
                s.agent.as_str()
            ));
        }
    }
    let connected = integrations.iter().any(|s| {
        matches!(
            s.state,
            IntegrationState::Connected | IntegrationState::AwaitingFirstEvent
        )
    });
    if connected && !hook_bin.is_file() {
        problems.push(format!(
            "Hook executable missing at {}. Re-run setup.",
            hook_bin.display()
        ));
    }

    if out.json {
        ui::print_json(&serde_json::json!({
            "ok": problems.is_empty(),
            "version": env!("CARGO_PKG_VERSION"),
            "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "dataDir": paths.root,
            "hookExecutable": hook_bin,
            "collector": { "running": client.is_some(), "healthLatencyMs": health_ms, "diagnostics": diag },
            "spool": { "files": spool_files, "bytes": spool_bytes },
            "integrations": integrations,
            "problems": problems,
        }))?;
        return Ok(if problems.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }

    println!(
        "{}",
        bold(&format!(
            "runmarq {} on {}-{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    );
    println!("  Data directory   {}", paths.root.display());
    println!(
        "  Hook executable  {} {}",
        hook_bin.display(),
        mark(hook_bin.is_file())
    );
    println!();
    println!("{}", bold("Collector"));
    match (&client, &diag) {
        (Some(c), Some(d)) => {
            println!(
                "  {} Running, pid {}, 127.0.0.1:{}, up {} min",
                mark(true),
                d.pid,
                c.discovery.port,
                d.uptime_seconds / 60
            );
            println!("  Health round trip   {:.1} ms", health_ms.unwrap_or(0.0));
            let fmt = |l: &runmarq_core::api::LatencyStats| match (l.p50_ms, l.p95_ms) {
                (Some(a), Some(b)) => {
                    format!("p50 {a:.1} ms, p95 {b:.1} ms ({} samples)", l.samples)
                }
                _ => "no samples yet".into(),
            };
            println!("  Hook processing     {}", fmt(&d.hook_latency));
            println!("  Ingest              {}", fmt(&d.ingest_latency));
            println!("  Forecast            {}", fmt(&d.forecast_latency));
            println!("  Model training      {}", fmt(&d.training_latency));
            println!(
                "  Events              {} received, {} duplicates, {} rejected",
                d.events_received, d.events_duplicate, d.events_rejected
            );
            println!(
                "  Database            {:.1} MB",
                d.database_bytes as f64 / 1_048_576.0
            );
            for e in &d.recent_errors {
                println!("  {} {e}", ui::yellow("recent:"));
            }
        }
        _ => println!("  {} Not running", mark(false)),
    }
    println!("  Spool               {spool_files} file(s), {spool_bytes} bytes pending");
    println!();
    println!("{}", bold("Integrations"));
    for s in &integrations {
        println!("  {:<12} {}", s.display_name, state_label(s.state));
        if matches!(
            s.state,
            IntegrationState::NotConnected | IntegrationState::NotDetected
        ) {
            if s.detected {
                println!(
                    "    {}",
                    dim(&format!(
                        "Set up with `runmarq integrations setup {}`",
                        s.agent.as_str()
                    ))
                );
            }
            continue;
        }
        for c in s.checks.iter().filter(|c| !c.ok) {
            println!(
                "    {} {}{}",
                mark(false),
                c.label,
                c.detail
                    .as_ref()
                    .map(|d| dim(&format!(" ({d})")))
                    .unwrap_or_default()
            );
        }
        for p in &s.pending_steps {
            println!("    {} {p}", ui::yellow("next:"));
        }
    }
    println!();
    if problems.is_empty() {
        println!("{} No problems found.", mark(true));
        Ok(ExitCode::SUCCESS)
    } else {
        for p in &problems {
            println!("{} {p}", mark(false));
        }
        Ok(ExitCode::from(1))
    }
}
