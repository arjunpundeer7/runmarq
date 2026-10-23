//! `runmarq integrations ...`

use crate::ui::{self, bold, dim, mark};
use crate::{IntegrationsCmd, Output, ScopeArgs, SetupArgs};
use anyhow::{anyhow, Context, Result};
use runmarq_adapters::{binary, for_source, Adapter, Scope};
use runmarq_core::api::{IntegrationState, IntegrationStatus};
use runmarq_core::event::AgentSource;
use runmarq_core::paths::Paths;
use std::process::ExitCode;

pub fn parse_agent(s: &str) -> Result<AgentSource> {
    AgentSource::parse(s)
        .ok_or_else(|| anyhow!("unknown agent '{s}'; expected claude, codex or cursor"))
}

fn scope(args: &ScopeArgs) -> Result<Scope> {
    Ok(match &args.project {
        Some(p) => Scope::Project {
            root: std::fs::canonicalize(p)
                .with_context(|| format!("project directory {}", p.display()))?,
        },
        None => Scope::User,
    })
}

/// The executable hooks should call: the stable copy in the data directory.
pub fn hook_executable(paths: &Paths, use_current: bool) -> Result<std::path::PathBuf> {
    let current = std::env::current_exe().context("locating the runmarq executable")?;
    if use_current {
        return Ok(current);
    }
    binary::ensure_installed(&current, &paths.root).context("installing the hook executable")
}

pub use runmarq_collector::integrations::options;

pub fn statuses(paths: &Paths, scope: &Scope) -> Vec<IntegrationStatus> {
    let last = runmarq_collector::integrations::last_events(paths, None);
    runmarq_collector::integrations::statuses(paths, scope, &last)
}

fn dirs_home() -> std::path::PathBuf {
    runmarq_collector::integrations::home_dir()
}

pub fn state_label(s: IntegrationState) -> String {
    match s {
        IntegrationState::NotDetected => dim("not detected"),
        IntegrationState::NotConnected => "not connected".into(),
        IntegrationState::AwaitingFirstEvent => ui::yellow("configured, waiting for first event"),
        IntegrationState::Connected => ui::green("connected"),
        IntegrationState::Limited => ui::yellow("limited"),
        IntegrationState::Error => ui::red("needs attention"),
    }
}

fn record(
    paths: &Paths,
    adapter: &dyn Adapter,
    scope: &Scope,
    config: &std::path::Path,
    status: &str,
) {
    runmarq_collector::integrations::record_installation(paths, adapter, scope, config, status);
}

fn setup(paths: &Paths, args: SetupArgs) -> Result<ExitCode> {
    let agent = parse_agent(&args.agent)?;
    let adapter = for_source(agent);
    let scope = scope(&args.scope)?;
    let exe = if args.dry_run {
        if args.use_current_exe {
            std::env::current_exe()?
        } else {
            binary::installed_path(&paths.root)
        }
    } else {
        hook_executable(paths, args.use_current_exe)?
    };
    let opts = options(paths, scope.clone(), exe);
    let det = adapter.detect(&dirs_home());
    if !det.detected {
        eprintln!("{} {} was not found on this machine. Setup will continue, but no events will arrive until it is installed.",
            ui::yellow("note:"), agent.display_name());
    }
    let plan = adapter.plan(&opts)?;
    if args.out.json && (args.dry_run || !args.yes) {
        ui::print_json(&plan)?;
        if args.dry_run {
            return Ok(ExitCode::SUCCESS);
        }
    } else if !args.out.json {
        println!("{}", bold(&format!("Set up {}", agent.display_name())));
        println!("  File     {}", plan.config_path.display());
        println!("  Command  {}", plan.command);
        println!("  Events   {}", plan.events.join(", "));
        for n in &plan.notes {
            println!("  {} {n}", dim("•"));
        }
        if plan.already_installed {
            println!("\n{} Already installed; nothing to change.", mark(true));
        } else {
            println!("\n{}", dim("runmarq adds these entries:"));
            println!("{}", serde_json::to_string_pretty(&plan.preview)?);
        }
        if args.dry_run {
            println!("\n{}", dim("Dry run: nothing was written."));
            return Ok(ExitCode::SUCCESS);
        }
    }
    if !plan.already_installed
        && !args.yes
        && !ui::confirm(&format!(
            "Write these changes to {}?",
            plan.config_path.display()
        ))
    {
        eprintln!("Setup cancelled. Re-run with --yes to apply without a prompt.");
        return Ok(ExitCode::from(2));
    }
    let result = adapter.install(&opts)?;
    record(
        paths,
        adapter.as_ref(),
        &scope,
        &result.config_path,
        "installed",
    );
    let ver = adapter.verify(&opts);
    if args.out.json {
        ui::print_json(&serde_json::json!({ "result": result, "verification": ver }))?;
    } else {
        if let Some(b) = &result.backup_path {
            println!(
                "{} Backup of the previous file: {}",
                mark(true),
                b.display()
            );
        }
        for c in &ver.checks {
            println!(
                "{} {}{}",
                mark(c.ok),
                c.label,
                c.detail
                    .as_ref()
                    .map(|d| dim(&format!(" ({d})")))
                    .unwrap_or_default()
            );
        }
        for step in &result.pending_steps {
            println!("{} {step}", ui::yellow("next:"));
        }
        if ver.complete {
            println!(
                "\n{} is configured. It shows as connected after its first event arrives; start a new session and send a prompt.",
                agent.display_name()
            );
        }
    }
    Ok(if ver.complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn print_status(s: &IntegrationStatus, verbose: bool) {
    println!(
        "{:<12} {:<36} {}",
        bold(&s.display_name),
        state_label(s.state),
        dim(&s
            .installed_version
            .clone()
            .map(|v| format!("v{v}"))
            .unwrap_or_default())
    );
    if verbose {
        for c in &s.checks {
            println!(
                "  {} {}{}",
                mark(c.ok),
                c.label,
                c.detail
                    .as_ref()
                    .map(|d| dim(&format!(" ({d})")))
                    .unwrap_or_default()
            );
        }
        for p in &s.pending_steps {
            println!("  {} {p}", ui::yellow("next:"));
        }
    }
}

pub fn run(paths: &Paths, cmd: IntegrationsCmd) -> Result<ExitCode> {
    match cmd {
        IntegrationsCmd::List(Output { json }) => {
            let all = statuses(paths, &Scope::User);
            if json {
                ui::print_json(&all)?;
            } else {
                for s in &all {
                    print_status(s, false);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        IntegrationsCmd::Setup(args) => setup(paths, args),
        IntegrationsCmd::Verify {
            agent,
            scope: sc,
            out,
        } => {
            let sc = scope(&sc)?;
            let filter = agent.as_deref().map(parse_agent).transpose()?;
            let all: Vec<_> = statuses(paths, &sc)
                .into_iter()
                .filter(|s| filter.is_none_or(|a| s.agent == a))
                .collect();
            let bad = all.iter().any(|s| s.state == IntegrationState::Error);
            if out.json {
                ui::print_json(&all)?;
            } else {
                for s in &all {
                    print_status(s, true);
                }
            }
            Ok(if bad {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        IntegrationsCmd::Remove {
            agent,
            scope: sc,
            yes,
            out,
        } => {
            let agent = parse_agent(&agent)?;
            let adapter = for_source(agent);
            let sc = scope(&sc)?;
            let opts = options(paths, sc.clone(), binary::installed_path(&paths.root));
            let path = adapter.config_path(&sc, &opts.home());
            if !yes
                && !out.json
                && !ui::confirm(&format!("Remove runmarq hooks from {}?", path.display()))
            {
                eprintln!("Nothing removed.");
                return Ok(ExitCode::from(2));
            }
            let r = adapter.uninstall(&opts)?;
            record(paths, adapter.as_ref(), &sc, &r.config_path, "removed");
            if out.json {
                ui::print_json(&r)?;
            } else if r.removed_entries == 0 {
                println!("No runmarq hooks found in {}.", r.config_path.display());
            } else {
                println!(
                    "{} Removed {} runmarq hook entries from {}.",
                    mark(true),
                    r.removed_entries,
                    r.config_path.display()
                );
                if let Some(b) = r.backup_path {
                    println!("  Backup: {}", b.display());
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        IntegrationsCmd::Manifest => {
            ui::print_json(&runmarq_adapters::manifest())?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
