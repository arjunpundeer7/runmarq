//! `runmarq`: hook ingestion, agent wrapper, integration management and
//! collector control. Every command supports `--json` for machine-readable
//! output and exits nonzero on failure, except `emit`, which always exits 0
//! so a runmarq problem can never interfere with an agent.

mod collector_cmd;
mod emit;
mod eval;
mod integrations;
mod status;
mod statusline;
mod ui;
mod wait;
mod wrapper;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "runmarq",
    version,
    about = "Know when your coding agent will finish.",
    long_about = "runmarq watches Claude Code, Codex and Cursor runs through their official hook interfaces, \
                  estimates how long each run will take, and tells you when it finishes or needs you.\n\n\
                  Everything stays on this machine. Prompts and source code are never stored or uploaded."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone, Copy, Default)]
pub struct Output {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Show the collector state and active runs.
    Status(Output),
    /// Check the installation, collector, integrations and hook latency.
    Doctor(Output),
    /// Manage agent integrations.
    #[command(subcommand)]
    Integrations(IntegrationsCmd),
    /// Control the local collector.
    #[command(subcommand)]
    Collector(CollectorCmd),
    /// List recent runs.
    History {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[command(flatten)]
        out: Output,
    },
    /// Export run history (timing metadata only) as CSV.
    Export {
        /// Write to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Replay recorded runs to see how the estimates would have held up.
    Eval(Output),
    /// Delete stored run history.
    Forget {
        /// Delete only runs that started before this date (YYYY-MM-DD).
        #[arg(long)]
        before: Option<String>,
        /// Confirm deletion without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Run an agent under runmarq, preserving its terminal, signals and exit code.
    ///
    /// Example: runmarq run -- codex
    Run {
        /// The command to run, after `--`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Wait for the run in progress to finish, then exit with its outcome.
    Wait(wait::WaitArgs),
    /// Print one short status line for a shell prompt, tmux or Claude Code.
    Statusline(statusline::StatuslineArgs),
    /// Receive one hook event on stdin and forward it to the collector.
    ///
    /// Called by agent hooks. Prints only what the agent's hook protocol
    /// requires and always exits 0.
    Emit {
        /// Which agent sent the event: claude_code, codex or cursor.
        #[arg(long)]
        source: String,
        /// Hook event name, if the payload does not include one.
        #[arg(long)]
        event: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum IntegrationsCmd {
    /// Show every supported agent and its integration state.
    List(Output),
    /// Install runmarq hooks for an agent.
    Setup(SetupArgs),
    /// Verify installed integrations.
    Verify {
        /// Limit to one agent.
        agent: Option<String>,
        #[command(flatten)]
        scope: ScopeArgs,
        #[command(flatten)]
        out: Output,
    },
    /// Remove runmarq hooks for an agent, leaving all other configuration untouched.
    Remove {
        agent: String,
        #[command(flatten)]
        scope: ScopeArgs,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        out: Output,
    },
    /// Print the public capability manifest.
    Manifest,
}

#[derive(Args, Clone)]
pub struct ScopeArgs {
    /// Configure one project instead of every project for this user.
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
}

#[derive(Args)]
pub struct SetupArgs {
    /// claude, codex or cursor.
    pub agent: String,
    #[command(flatten)]
    pub scope: ScopeArgs,
    /// Show the change without writing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Apply without the confirmation prompt.
    #[arg(long)]
    pub yes: bool,
    /// Point hooks at this executable instead of the stable copy in the data directory.
    #[arg(long, hide = true)]
    pub use_current_exe: bool,
    #[command(flatten)]
    pub out: Output,
}

#[derive(Subcommand)]
pub enum CollectorCmd {
    /// Start the collector in the background (or in this terminal with --foreground).
    Start {
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the running collector.
    Stop,
    /// Show whether the collector is running.
    Status(Output),
}

fn main() -> ExitCode {
    // `emit` must start fast and stay silent; dispatch it before anything else.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("emit") {
        emit::main_from_args(&args[2..]);
        return ExitCode::SUCCESS;
    }

    let cli = Cli::parse();
    // The status line runs on every prompt: it never migrates data or fails.
    if let Command::Statusline(args) = &cli.command {
        return statusline::run(&runmarq_core::paths::Paths::resolve(), args);
    }
    let current = std::env::current_exe().ok();
    let (paths, moved) = runmarq_collector::migrate::resolve_migrating(current.as_deref());
    if let Some(m) = moved {
        eprintln!(
            "{} Moved runmarq data from {} to {}; updated hooks in {} file(s).",
            ui::mark(m.problems.is_empty()),
            m.from.display(),
            m.to.display(),
            m.rewritten.len()
        );
        for p in &m.problems {
            eprintln!("  {p}");
        }
    }
    let result = match cli.command {
        Command::Status(out) => status::status(&paths, out),
        Command::Doctor(out) => status::doctor(&paths, out),
        Command::Integrations(cmd) => integrations::run(&paths, cmd),
        Command::Collector(cmd) => collector_cmd::run(&paths, cmd),
        Command::History { limit, out } => status::history(&paths, limit, out),
        Command::Export { output } => status::export(&paths, output),
        Command::Eval(out) => eval::eval(&paths, out),
        Command::Forget { before, yes } => status::forget(&paths, before, yes),
        Command::Run { command } => wrapper::run(&paths, command),
        Command::Wait(args) => wait::run(&paths, args),
        Command::Statusline(_) | Command::Emit { .. } => unreachable!("handled above"),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e:#}", ui::red("error:"));
            ExitCode::from(1)
        }
    }
}
