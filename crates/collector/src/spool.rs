//! Bounded on-disk spool for events emitted while the collector is down.
//!
//! - Contains only normalized, minimized envelopes (never raw payloads).
//! - Files are created with `create_new` (and `O_NOFOLLOW` on Unix), so a
//!   planted symlink cannot redirect writes.
//! - Total size and file count are capped; when full, new events are dropped
//!   and counted rather than growing without bound.

use crate::lifecycle::private_file;
use runmarq_core::event::EventEnvelope;
use runmarq_core::paths::Paths;
use std::io::Write;
use std::path::PathBuf;

pub const MAX_SPOOL_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_SPOOL_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum SpoolResult {
    Written,
    Full,
    Failed,
}

fn spool_files(paths: &Paths) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(paths.spool_dir()) else {
        return vec![];
    };
    let mut files: Vec<(PathBuf, u64)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            let name = e.file_name();
            let name = name.to_str()?;
            (m.is_file() && name.ends_with(".jsonl")).then(|| (e.path(), m.len()))
        })
        .collect();
    files.sort();
    files
}

pub fn pending(paths: &Paths) -> (usize, u64) {
    let f = spool_files(paths);
    (f.len(), f.iter().map(|(_, s)| s).sum())
}

pub fn write(paths: &Paths, events: &[EventEnvelope]) -> SpoolResult {
    if events.is_empty() {
        return SpoolResult::Written;
    }
    let dir = paths.spool_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return SpoolResult::Failed;
    }
    let (count, bytes) = pending(paths);
    let mut body = Vec::new();
    for e in events {
        if let Ok(line) = serde_json::to_vec(e) {
            body.extend_from_slice(&line);
            body.push(b'\n');
        }
    }
    if count >= MAX_SPOOL_FILES || bytes + body.len() as u64 > MAX_SPOOL_BYTES {
        return SpoolResult::Full;
    }
    let name = format!(
        "{:013}-{}-{:08x}.jsonl",
        chrono::Utc::now().timestamp_millis(),
        std::process::id(),
        rand::random::<u32>()
    );
    match private_file(&dir.join(name), true).and_then(|mut f| f.write_all(&body)) {
        Ok(()) => SpoolResult::Written,
        Err(_) => SpoolResult::Failed,
    }
}

/// Reads spooled events in order. The caller deletes each file with
/// [`remove`] after its events have been ingested.
pub fn read_all(paths: &Paths) -> Vec<(PathBuf, Vec<EventEnvelope>)> {
    spool_files(paths)
        .into_iter()
        .map(|(p, size)| {
            let events = if size > MAX_FILE_BYTES {
                Vec::new() // never written by runmarq; discard
            } else {
                std::fs::read_to_string(&p)
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|l| serde_json::from_str::<EventEnvelope>(l).ok())
                    .collect()
            };
            (p, events)
        })
        .collect()
}

pub fn remove(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use runmarq_core::event::*;

    fn ev(i: u32) -> EventEnvelope {
        EventEnvelope {
            schema_version: SCHEMA_VERSION,
            event_id: format!("ev_{i}"),
            source: AgentSource::ClaudeCode,
            source_session_id: "s".into(),
            source_turn_id: None,
            timestamp: chrono::Utc::now(),
            timestamp_origin: TimestampOrigin::HookInvocation,
            event_type: EventType::Heartbeat,
            payload: EventPayload::default(),
        }
    }

    #[test]
    fn write_read_and_bound() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        assert_eq!(write(&paths, &[ev(1), ev(2)]), SpoolResult::Written);
        assert_eq!(write(&paths, &[ev(3)]), SpoolResult::Written);
        let all = read_all(&paths);
        assert_eq!(all.iter().map(|(_, e)| e.len()).sum::<usize>(), 3);
        for (p, _) in &all {
            remove(p);
        }
        assert_eq!(pending(&paths).0, 0);

        let big: Vec<_> = (0..40_000).map(ev).collect();
        let mut full = false;
        for _ in 0..10 {
            if write(&paths, &big) == SpoolResult::Full {
                full = true;
                break;
            }
        }
        assert!(full, "spool is bounded");
        assert!(pending(&paths).1 <= MAX_SPOOL_BYTES);
    }

    #[test]
    fn ignores_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        std::fs::create_dir_all(paths.spool_dir()).unwrap();
        std::fs::write(paths.spool_dir().join("notes.txt"), "x").unwrap();
        std::fs::write(paths.spool_dir().join("bad.jsonl"), "garbage\n{}\n").unwrap();
        let all = read_all(&paths);
        assert_eq!(all.len(), 1);
        assert!(all[0].1.is_empty());
    }
}
