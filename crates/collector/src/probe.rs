//! Reads signals that hooks do not carry: token counts from the agent's
//! transcript and the size of the working-tree diff. Both run outside the
//! engine lock. Transcript lines are parsed for numbers only; file paths
//! from `git diff` stay in memory and are never stored.

use chrono::{DateTime, Duration, Utc};
use runmarq_adapters::transcript::{parse_line, Entry, TranscriptFormat};
use runmarq_core::signals::RunSignals;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, Stdio};
use std::time::Instant;

const FIRST_READ_MAX: u64 = 16 * 1024 * 1024;
const READ_MAX: u64 = 8 * 1024 * 1024;
const MAX_MESSAGES: usize = 20_000;
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const GIT_MAX_OUTPUT: u64 = 4 * 1024 * 1024;
const MAX_DIFF_FILES: usize = 20_000;

#[derive(Debug, Clone, Copy, Default)]
struct Usage {
    output: u64,
    thinking: u64,
    sidechain: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Totals {
    output: u64,
    reasoning: u64,
}

/// Position in one transcript file and what has been read from it for the
/// current run. Entries from before the run started are skipped, except
/// Codex running totals, which set the baseline the run's usage is measured from.
#[derive(Debug, Clone)]
pub struct TranscriptCursor {
    pub path: PathBuf,
    format: TranscriptFormat,
    since: DateTime<Utc>,
    offset: Option<u64>,
    messages: HashMap<String, Usage>,
    anonymous: u64,
    base: Option<Totals>,
    last: Option<Totals>,
    responses: u32,
    context_tokens: u64,
    context_window: Option<u64>,
    compactions: u32,
    last_turn_at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl TranscriptCursor {
    pub fn new(path: PathBuf, format: TranscriptFormat, since: DateTime<Utc>) -> TranscriptCursor {
        TranscriptCursor {
            path,
            format,
            since: since - Duration::seconds(2),
            offset: None,
            messages: HashMap::new(),
            anonymous: 0,
            base: None,
            last: None,
            responses: 0,
            context_tokens: 0,
            context_window: None,
            compactions: 0,
            last_turn_at: None,
            model: None,
            effort: None,
        }
    }

    fn reset(&mut self) {
        let since = self.since;
        *self = TranscriptCursor::new(self.path.clone(), self.format, since);
        self.since = since;
    }

    /// Reads lines appended since the last poll. Returns whether anything
    /// relevant to this run was read.
    pub fn poll(&mut self) -> std::io::Result<bool> {
        let mut file = std::fs::File::open(&self.path)?;
        let len = file.metadata()?.len();
        if self.offset.is_some_and(|o| o > len) {
            self.reset();
        }
        let (start, skip_partial) = match self.offset {
            Some(o) => (o, false),
            None => {
                let s = len.saturating_sub(FIRST_READ_MAX);
                (s, s > 0)
            }
        };
        if start >= len {
            self.offset = Some(start);
            return Ok(false);
        }
        file.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        file.take(READ_MAX).read_to_end(&mut buf)?;
        let mut begin = 0;
        if skip_partial {
            match buf.iter().position(|b| *b == b'\n') {
                Some(i) => begin = i + 1,
                None => {
                    self.offset = Some(start + buf.len() as u64);
                    return Ok(false);
                }
            }
        }
        let end = match buf.iter().rposition(|b| *b == b'\n') {
            Some(i) if i >= begin => i + 1,
            _ if buf.len() as u64 >= READ_MAX => buf.len(),
            _ => {
                self.offset = Some(start + begin as u64);
                return Ok(false);
            }
        };
        let mut changed = false;
        for line in buf[begin..end].split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Ok(text) = std::str::from_utf8(line) else {
                continue;
            };
            if let Some(entry) = parse_line(self.format, text) {
                changed |= self.apply(entry);
            }
        }
        self.offset = Some(start + end as u64);
        Ok(changed)
    }

    fn apply(&mut self, entry: Entry) -> bool {
        let before = entry.at().is_some_and(|a| a < self.since);
        match entry {
            Entry::Message {
                id,
                at,
                output_tokens,
                thinking_tokens,
                context_tokens,
                model,
                effort,
                sidechain,
            } => {
                if before {
                    return false;
                }
                let key = match id {
                    Some(id) => id,
                    None => {
                        self.anonymous += 1;
                        format!("#{}", self.anonymous)
                    }
                };
                if self.messages.len() < MAX_MESSAGES || self.messages.contains_key(&key) {
                    let u = self.messages.entry(key).or_default();
                    u.output = u.output.max(output_tokens);
                    u.thinking = u.thinking.max(thinking_tokens);
                    u.sidechain = sidechain;
                }
                if !sidechain {
                    if context_tokens > 0 {
                        self.context_tokens = context_tokens;
                    }
                    self.last_turn_at = self.last_turn_at.max(at);
                    if model.is_some() {
                        self.model = model;
                    }
                }
                if effort.is_some() {
                    self.effort = effort;
                }
                true
            }
            Entry::Totals {
                at,
                output_tokens,
                reasoning_tokens,
                context_tokens,
                context_window,
            } => {
                let t = Totals {
                    output: output_tokens,
                    reasoning: reasoning_tokens,
                };
                if before {
                    self.base = Some(t);
                    return false;
                }
                if self.last != Some(t) {
                    self.responses += 1;
                }
                self.last = Some(t);
                if context_tokens > 0 {
                    self.context_tokens = context_tokens;
                }
                self.context_window = context_window.or(self.context_window);
                self.last_turn_at = self.last_turn_at.max(at);
                true
            }
            Entry::Context { model, effort, .. } => {
                if model.is_some() {
                    self.model = model;
                }
                if effort.is_some() {
                    self.effort = effort;
                }
                !before
            }
            Entry::Compaction { .. } => {
                if before {
                    return false;
                }
                self.compactions += 1;
                true
            }
        }
    }

    /// Writes the transcript-derived fields into `s`.
    pub fn fill(&self, s: &mut RunSignals) {
        match self.format {
            TranscriptFormat::Claude => {
                s.turns = self.messages.values().filter(|u| !u.sidechain).count() as u32;
                s.subagent_turns = self.messages.values().filter(|u| u.sidechain).count() as u32;
                s.output_tokens = self.messages.values().map(|u| u.output).sum();
                s.thinking_tokens = self.messages.values().map(|u| u.thinking).sum();
            }
            TranscriptFormat::Codex => {
                let base = self.base.unwrap_or_default();
                let last = self.last.unwrap_or(base);
                s.turns = self.responses;
                s.output_tokens = last.output.saturating_sub(base.output);
                s.thinking_tokens = last.reasoning.saturating_sub(base.reasoning);
            }
        }
        s.context_tokens = self.context_tokens;
        s.context_window = self.context_window.or(s.context_window);
        s.compactions = self.compactions;
        s.last_turn_at = self.last_turn_at;
    }
}

/// Placeholder counts for binary files, which `git diff --numstat` shows as `-`.
const BINARY: (u32, u32) = (u32::MAX, u32::MAX);

/// Lines added and removed per file in the working tree relative to HEAD.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiffStat(HashMap<String, (u32, u32)>);

impl DiffStat {
    pub fn parse(numstat: &str) -> DiffStat {
        let mut m = HashMap::new();
        for line in numstat.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(a), Some(r), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            if m.len() >= MAX_DIFF_FILES {
                break;
            }
            let counts = if a == "-" || r == "-" {
                BINARY
            } else {
                (a.parse().unwrap_or(0), r.parse().unwrap_or(0))
            };
            m.insert(path.to_string(), counts);
        }
        DiffStat(m)
    }

    /// Files and lines that changed since `base` was taken.
    pub fn growth(&self, base: &DiffStat) -> (u32, u32) {
        let lines_of = |c: (u32, u32)| if c == BINARY { (0, 0) } else { c };
        let mut files = 0u32;
        let mut lines = 0u64;
        for (path, &now) in &self.0 {
            let before = base.0.get(path).copied();
            if before != Some(now) {
                let (a, r) = lines_of(now);
                let (ba, br) = before.map(lines_of).unwrap_or((0, 0));
                files += 1;
                lines += a.abs_diff(ba) as u64 + r.abs_diff(br) as u64;
            }
        }
        for (path, &before) in &base.0 {
            if !self.0.contains_key(path) {
                let (ba, br) = lines_of(before);
                files += 1;
                lines += ba as u64 + br as u64;
            }
        }
        (files, lines.min(u32::MAX as u64) as u32)
    }
}

fn run_git<T: Send + 'static>(
    cwd: &Path,
    args: &[&str],
    read: impl FnOnce(ChildStdout) -> T + Send + 'static,
) -> Option<T> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(cwd)
        .args([
            "--no-pager",
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "diff.external=",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || read(stdout));
    let deadline = Instant::now() + GIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(15))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn diff_stat(cwd: &Path) -> Option<DiffStat> {
    let out = run_git(
        cwd,
        &[
            "diff",
            "--numstat",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "HEAD",
            "--",
        ],
        |mut s| {
            let mut buf = Vec::new();
            let _ = (&mut s).take(GIT_MAX_OUTPUT).read_to_end(&mut buf);
            let mut sink = [0u8; 8192];
            while matches!(s.read(&mut sink), Ok(n) if n > 0) {}
            buf
        },
    )?;
    Some(DiffStat::parse(&String::from_utf8_lossy(&out)))
}

pub fn tracked_files(cwd: &Path) -> Option<u32> {
    run_git(cwd, &["ls-files", "-z"], |mut s| {
        let mut n = 0u64;
        let mut buf = [0u8; 64 * 1024];
        while let Ok(k) = s.read(&mut buf) {
            if k == 0 {
                break;
            }
            n += buf[..k].iter().filter(|b| **b == 0).count() as u64;
        }
        n.min(u32::MAX as u64) as u32
    })
}

#[derive(Debug, Clone)]
pub struct GitProbe {
    pub cwd: PathBuf,
    pub baseline: Option<DiffStat>,
    pub count_files: bool,
}

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub run_id: String,
    pub repository_id: Option<String>,
    pub transcript: Option<TranscriptCursor>,
    pub git: Option<GitProbe>,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub run_id: String,
    pub repository_id: Option<String>,
    pub transcript: Option<TranscriptCursor>,
    pub transcript_changed: bool,
    pub baseline: Option<DiffStat>,
    /// Files and lines changed since the baseline.
    pub diff: Option<(u32, u32)>,
    pub file_count: Option<u32>,
}

pub fn run(req: ProbeRequest) -> ProbeResult {
    let mut transcript = req.transcript;
    let transcript_changed = match transcript.as_mut() {
        Some(t) => t.poll().unwrap_or(false),
        None => false,
    };
    let mut out = ProbeResult {
        run_id: req.run_id,
        repository_id: req.repository_id,
        transcript,
        transcript_changed,
        baseline: None,
        diff: None,
        file_count: None,
    };
    if let Some(g) = req.git {
        if let Some(now) = diff_stat(&g.cwd) {
            match g.baseline {
                Some(base) => {
                    out.diff = Some(now.growth(&base));
                    out.baseline = Some(base);
                }
                None => {
                    out.diff = Some((0, 0));
                    out.baseline = Some(now);
                }
            }
        } else {
            out.baseline = g.baseline;
        }
        if g.count_files {
            out.file_count = tracked_files(&g.cwd);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn claude_line(id: &str, at: &str, out: u64, ctx: u64, side: bool) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{at}","isSidechain":{side},"message":{{"id":"{id}","model":"claude-x","usage":{{"input_tokens":{ctx},"output_tokens":{out}}}}}}}"#
        )
    }

    #[test]
    fn claude_transcripts_count_this_run_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "{}",
            claude_line("old", "2026-01-01T09:00:00Z", 999, 5, false)
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            claude_line("m1", "2026-01-01T10:00:01Z", 10, 100, false)
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            claude_line("m1", "2026-01-01T10:00:02Z", 40, 100, false)
        )
        .unwrap();
        write!(
            f,
            "{}",
            claude_line("m2", "2026-01-01T10:00:03Z", 7, 50, true)
        )
        .unwrap();
        drop(f);

        let mut c = TranscriptCursor::new(
            path.clone(),
            TranscriptFormat::Claude,
            ts("2026-01-01T10:00:00Z"),
        );
        assert!(c.poll().unwrap());
        let mut s = RunSignals::default();
        c.fill(&mut s);
        assert_eq!(
            (s.turns, s.subagent_turns, s.output_tokens),
            (1, 0, 40),
            "the partial last line waits"
        );
        assert_eq!(s.context_tokens, 100);

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f).unwrap();
        writeln!(
            f,
            r#"{{"type":"system","subtype":"compact_boundary","timestamp":"2026-01-01T10:00:04Z"}}"#
        )
        .unwrap();
        drop(f);
        assert!(c.poll().unwrap());
        c.fill(&mut s);
        assert_eq!(
            (s.turns, s.subagent_turns, s.output_tokens, s.compactions),
            (1, 1, 47, 1)
        );
        assert!(!c.poll().unwrap(), "nothing new");
    }

    #[test]
    fn codex_usage_is_measured_from_the_run_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let tok = |at: &str, out: u64| {
            format!(
                r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"output_tokens":{out},"reasoning_output_tokens":0}},"last_token_usage":{{"input_tokens":900}},"model_context_window":200000}}}}}}"#
            )
        };
        let body = [
            tok("2026-01-01T09:00:00Z", 500),
            tok("2026-01-01T10:00:05Z", 650),
            tok("2026-01-01T10:00:09Z", 700),
        ]
        .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();
        let mut c =
            TranscriptCursor::new(path, TranscriptFormat::Codex, ts("2026-01-01T10:00:00Z"));
        assert!(c.poll().unwrap());
        let mut s = RunSignals::default();
        c.fill(&mut s);
        assert_eq!(
            (s.turns, s.output_tokens, s.context_tokens, s.context_window),
            (2, 200, 900, Some(200_000))
        );
    }

    #[test]
    fn truncated_files_start_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(
            &path,
            claude_line("a", "2026-01-01T10:00:01Z", 5, 1, false)
                + "\n"
                + &claude_line("b", "2026-01-01T10:00:02Z", 5, 1, false)
                + "\n",
        )
        .unwrap();
        let mut c = TranscriptCursor::new(
            path.clone(),
            TranscriptFormat::Claude,
            ts("2026-01-01T10:00:00Z"),
        );
        c.poll().unwrap();
        std::fs::write(
            &path,
            claude_line("c", "2026-01-01T10:00:03Z", 9, 1, false) + "\n",
        )
        .unwrap();
        c.poll().unwrap();
        let mut s = RunSignals::default();
        c.fill(&mut s);
        assert_eq!((s.turns, s.output_tokens), (1, 9));
    }

    #[test]
    fn diff_growth_is_relative_to_the_baseline() {
        let base = DiffStat::parse("3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n");
        let now =
            DiffStat::parse("5\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n4\t4\tsrc/c.rs\n-\t-\tlogo.png\n");
        assert_eq!(now.growth(&base), (3, 10));
        assert_eq!(base.growth(&base), (0, 0));
        assert_eq!(
            DiffStat::default().growth(&base),
            (2, 14),
            "reverted changes count too"
        );
    }

    #[test]
    fn git_probe_reads_a_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if !git(&["init", "-q"]) {
            return;
        }
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git(&["add", "a.txt"]);
        if !git(&["commit", "-q", "-m", "init"]) {
            return;
        }
        let base = diff_stat(dir.path()).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let now = diff_stat(dir.path()).unwrap();
        assert_eq!(now.growth(&base), (1, 2));
        assert_eq!(tracked_files(dir.path()), Some(1));
        assert!(diff_stat(&dir.path().join("missing")).is_none());
    }
}
