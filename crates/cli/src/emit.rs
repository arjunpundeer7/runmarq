//! The hook path: `runmarq emit --source <agent> [--event <name>]`.
//!
//! Contract with the agent:
//! - Reads the hook payload from stdin (bounded).
//! - Writes to stdout exactly what the vendor's hook protocol requires
//!   (nothing for Claude Code and Codex, a fixed JSON acknowledgement for
//!   Cursor), and nothing else, ever.
//! - Never writes to stderr; diagnostics go to a size-capped log file.
//! - Always exits 0 within a bounded time, even if the collector is down.
//!
//! The raw payload, including any prompt text, exists only in this process's
//! memory. Only normalized, minimized events are sent or spooled.

use chrono::Utc;
use runmarq_adapters::{for_source, NormalizeContext};
use runmarq_collector::{lifecycle, process, spool, Client};
use runmarq_core::api::{HookConfig, IngestRequest};
use runmarq_core::event::{AgentSource, EventType};
use runmarq_core::paths::Paths;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const MAX_STDIN_BYTES: u64 = 1024 * 1024;
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Upper bound for the network part of the hook path.
const SEND_TIMEOUT: Duration = Duration::from_millis(1000);

pub const LAUNCHER_ENV: &str = "RUNMARQ_LAUNCHER_ID";
pub const SUPPRESS_ENV: &str = "RUNMARQ_SUPPRESS_HOOKS";
pub const CAPTURE_ENV: &str = "RUNMARQ_CAPTURE_DIR";

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
    }
    None
}

pub fn log(paths: &Paths, msg: &str) {
    let p = paths.logs_dir().join("hook.log");
    let _ = std::fs::create_dir_all(paths.logs_dir());
    if std::fs::metadata(&p)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&p, paths.logs_dir().join("hook.log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "{} {msg}", Utc::now().to_rfc3339());
    }
}

fn hook_config(paths: &Paths) -> HookConfig {
    std::fs::read(paths.hook_config())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| HookConfig::from_preferences(&runmarq_core::api::Preferences::default()))
}

fn capture(paths: &Paths, source: AgentSource, raw: &[u8]) {
    let Some(dir) = std::env::var_os(CAPTURE_ENV) else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let name = format!(
        "{}-{}-{:08x}.json",
        source.as_str(),
        Utc::now().timestamp_millis(),
        rand::random::<u32>()
    );
    if let Err(e) = std::fs::write(dir.join(name), raw) {
        log(paths, &format!("capture failed: {e}"));
    }
}

pub fn main_from_args(args: &[String]) {
    let started = Instant::now();
    let now = Utc::now();
    let source = arg_value(args, "--source").and_then(|s| AgentSource::parse(&s));
    let declared = arg_value(args, "--event");

    let mut raw = Vec::new();
    let _ = std::io::stdin()
        .take(MAX_STDIN_BYTES + 1)
        .read_to_end(&mut raw);
    let oversized = raw.len() as u64 > MAX_STDIN_BYTES;
    let raw = decode_stdin(raw);

    let paths = Paths::resolve();
    let Some(source) = source else {
        log(&paths, "emit called without a valid --source");
        return;
    };
    let adapter = for_source(source);

    if std::env::var_os(SUPPRESS_ENV).is_some() || oversized {
        // Still acknowledge, so the agent's protocol is satisfied.
        let n = adapter.normalize(&placeholder_ctx(now), declared.as_deref(), b"{}");
        respond(n.stdout);
        if oversized {
            log(
                &paths,
                &format!("{source}: payload over {MAX_STDIN_BYTES} bytes ignored"),
            );
        }
        return;
    }
    capture(&paths, source, &raw);

    let ctx = NormalizeContext {
        now,
        hook_config: hook_config(&paths),
        salt: lifecycle::install_salt(&paths),
        launcher_id: std::env::var(LAUNCHER_ENV).ok().filter(|v| v.len() <= 64),
    };
    let mut normalized = adapter.normalize(&ctx, declared.as_deref(), &raw);
    drop(raw);
    respond(normalized.stdout);
    let starts = |t: EventType| matches!(t, EventType::PromptSubmitted | EventType::SessionStarted);
    if normalized.events.iter().any(|e| starts(e.event_type)) {
        let agent = process::agent_ancestor();
        for e in normalized
            .events
            .iter_mut()
            .filter(|e| starts(e.event_type))
        {
            e.payload.agent_process = agent;
        }
    }

    if let Some(reason) = &normalized.ignored {
        if reason != "cursor_compat_hook" && !reason.starts_with("notification:") {
            log(&paths, &format!("{source}: ignored payload ({reason})"));
        }
    }
    if normalized.events.is_empty() {
        return;
    }

    let latency = started.elapsed().as_secs_f64() * 1000.0;
    let req = IngestRequest {
        events: normalized.events,
        hook_latency_ms: Some(latency),
        replayed: false,
    };
    let sent = Client::discover(&paths, SEND_TIMEOUT)
        .map_err(|e| e.to_string())
        .and_then(|c| c.ingest(&req).map_err(|e| e.to_string()));
    if let Err(e) = sent {
        match spool::write(&paths, &req.events) {
            spool::SpoolResult::Written => {}
            spool::SpoolResult::Full => log(
                &paths,
                &format!(
                    "collector unavailable ({e}); spool full, {} event(s) dropped",
                    req.events.len()
                ),
            ),
            spool::SpoolResult::Failed => log(
                &paths,
                &format!("collector unavailable ({e}); could not spool"),
            ),
        }
    }
}

/// Normalizes stdin to UTF-8 without a byte-order mark. Windows PowerShell
/// (which some hook runners use) can prepend a BOM or send UTF-16 when
/// piping text to a native program; JSON parsers reject both.
pub fn decode_stdin(raw: Vec<u8>) -> Vec<u8> {
    let utf16 = |bytes: &[u8], le: bool| -> Vec<u8> {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units).into_bytes()
    };
    match raw.as_slice() {
        [0xEF, 0xBB, 0xBF, rest @ ..] => rest.to_vec(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        // UTF-16LE without a BOM: ASCII JSON starts with '{' followed by a NUL.
        [b'{', 0, ..] => utf16(&raw, true),
        _ => raw,
    }
}

fn placeholder_ctx(now: chrono::DateTime<Utc>) -> NormalizeContext {
    NormalizeContext {
        now,
        hook_config: HookConfig::default(),
        salt: Vec::new(),
        launcher_id: None,
    }
}

fn respond(stdout: Option<&'static str>) {
    if let Some(s) = stdout {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::arg_value;

    #[test]
    fn decodes_powershell_encodings() {
        use super::decode_stdin;
        let json = br#"{"a":1}"#.to_vec();
        assert_eq!(decode_stdin(json.clone()), json);
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(&json);
        assert_eq!(decode_stdin(bom), json);
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("{\"a\":1}".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_stdin(le), json);
        let le_nobom: Vec<u8> = "{\"a\":1}"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_stdin(le_nobom), json);
    }

    #[test]
    fn parses_args() {
        let a: Vec<String> = ["--source", "cursor", "--event=stop"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arg_value(&a, "--source").as_deref(), Some("cursor"));
        assert_eq!(arg_value(&a, "--event").as_deref(), Some("stop"));
        assert_eq!(arg_value(&a, "--missing"), None);
    }
}
