//! Prints the numbers runmarq reads from an agent transcript, and nothing else.
//!
//!     cargo run -p runmarq-collector --example transcript_numbers -- <file.jsonl> [claude|codex] [since]
//!
//! `since` is an RFC 3339 time; entries before it are skipped, as they are for
//! a run that started then. It defaults to the beginning of the file.

use chrono::{DateTime, Utc};
use runmarq_adapters::transcript::TranscriptFormat;
use runmarq_collector::probe::TranscriptCursor;
use runmarq_core::signals::RunSignals;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: transcript_numbers <file.jsonl> [claude|codex] [since]");
        std::process::exit(2);
    };
    let format = match args.next().as_deref() {
        Some("codex") => TranscriptFormat::Codex,
        _ => TranscriptFormat::Claude,
    };
    let since = args
        .next()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| {
            DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                .expect("valid")
                .with_timezone(&Utc)
        });
    let mut cursor = TranscriptCursor::new(path.into(), format, since);
    let started = Instant::now();
    let polls = 50;
    for _ in 0..polls {
        if let Err(e) = cursor.poll() {
            eprintln!("could not read the transcript: {e}");
            std::process::exit(1);
        }
    }
    let mut s = RunSignals::default();
    cursor.fill(&mut s);
    println!(
        "{}",
        serde_json::json!({
            "polls": polls,
            "ms": started.elapsed().as_millis() as u64,
            "turns": s.turns,
            "subagentTurns": s.subagent_turns,
            "outputTokens": s.output_tokens,
            "thinkingTokens": s.thinking_tokens,
            "contextTokens": s.context_tokens,
            "contextWindow": s.context_window,
            "compactions": s.compactions,
            "effort": cursor.effort,
            "modelReported": cursor.model.is_some(),
        })
    );
}
