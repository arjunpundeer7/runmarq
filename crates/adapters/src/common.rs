//! Helpers shared by adapters: classification, identity, minimization.
//!
//! Everything here treats vendor payloads as untrusted input. Strings are
//! length-bounded and character-filtered before they enter an event, and
//! sensitive fields (prompt text, commands, tool input and output) are only
//! inspected in memory to derive categories, never copied out.

use chrono::{DateTime, Utc};
use runmarq_core::event::{
    derive_event_id, ActivityCategory, AgentSource, EditStats, EventEnvelope, EventPayload,
    EventType, PlanSnapshot, RepositoryRef, TimestampOrigin, SCHEMA_VERSION,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::NormalizeContext;

/// Characters allowed in identifiers that come from vendors.
fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.:@".contains(c)
}

/// Returns a safe identifier: the vendor value if it is already clean and
/// short, otherwise a stable hash of it.
pub fn safe_id(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.len() <= 128 && raw.chars().all(id_char) {
        return Some(raw.to_string());
    }
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    Some(format!("h_{}", hex::encode(&h.finalize()[..12])))
}

/// Tool identifiers keep only safe characters and are bounded.
pub fn safe_tool_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.:".contains(*c))
        .take(64)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Short machine-readable keyword (status, reason), lowercase.
pub fn safe_keyword(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(48)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

pub fn safe_text(raw: &str, max: usize) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).take(max).collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

pub fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Extracts a shell command string from common tool input shapes, for
/// in-memory classification only.
pub fn command_text(input: Option<&Value>) -> Option<String> {
    let input = input?;
    match input.get("command").or_else(|| input.get("cmd")) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// Classifies a shell command. Matching is on lowercase tokens.
pub fn classify_command(cmd: &str) -> ActivityCategory {
    let c = cmd.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| c.contains(n));
    if has(&[
        "pytest",
        "jest",
        "vitest",
        "mocha",
        "rspec",
        "phpunit",
        "go test",
        "cargo test",
        "cargo nextest",
        "npm test",
        "npm run test",
        "pnpm test",
        "pnpm run test",
        "yarn test",
        "bun test",
        "dotnet test",
        "mvn test",
        "gradle test",
        "./gradlew test",
        "playwright test",
        "ctest",
        "tox",
        "unittest",
        "deno test",
        "mix test",
    ]) {
        return ActivityCategory::RunningTests;
    }
    if has(&[
        "npm install",
        "npm i ",
        "npm ci",
        "pnpm install",
        "pnpm add",
        "pnpm i ",
        "yarn add",
        "yarn install",
        "pip install",
        "pip3 install",
        "uv add",
        "uv sync",
        "poetry add",
        "poetry install",
        "cargo add",
        "cargo fetch",
        "go get",
        "go mod",
        "bundle install",
        "gem install",
        "brew install",
        "apt-get install",
        "apt install",
        "composer install",
        "composer require",
        "bun add",
        "bun install",
    ]) {
        return ActivityCategory::InstallingDependencies;
    }
    if has(&[
        "cargo build",
        "cargo check",
        "cargo clippy",
        "npm run build",
        "pnpm build",
        "pnpm run build",
        "yarn build",
        "tsc",
        "next build",
        "vite build",
        "webpack",
        "make",
        "cmake",
        "go build",
        "mvn package",
        "gradle build",
        "dotnet build",
        "msbuild",
        "bazel build",
        "tauri build",
        "turbo build",
        "turbo run build",
        "esbuild",
    ]) {
        return ActivityCategory::Building;
    }
    if has(&["gdb ", "lldb", "--inspect", "pdb", "strace", "dlv "]) {
        return ActivityCategory::Debugging;
    }
    let first = c.split_whitespace().next().unwrap_or("");
    let first = first.rsplit(['/', '\\']).next().unwrap_or(first);
    match first {
        "rg" | "grep" | "find" | "fd" | "ag" | "ls" | "dir" | "tree" | "git-grep"
        | "select-string" | "get-childitem" => ActivityCategory::SearchingRepository,
        "cat" | "head" | "tail" | "less" | "more" | "type" | "get-content" | "bat" | "sed" => {
            ActivityCategory::ReadingFiles
        }
        "curl" | "wget" => ActivityCategory::Research,
        _ => ActivityCategory::RunningCommands,
    }
}

/// Classifies a tool call by its name and (in memory) its input.
pub fn classify_tool(tool_name: &str, input: Option<&Value>) -> ActivityCategory {
    let n = tool_name.to_ascii_lowercase();
    if n.starts_with("mcp__") || n.starts_with("mcp:") || n.starts_with("mcp_") {
        return ActivityCategory::Research;
    }
    match n.as_str() {
        "read" | "notebookread" | "read_file" | "view" | "readfile" | "view_image" => {
            ActivityCategory::ReadingFiles
        }
        "edit" | "multiedit" | "write" | "notebookedit" | "apply_patch" | "applypatch"
        | "write_file" | "edit_file" | "str_replace" | "create_file" | "delete" | "delete_file"
        | "strreplace" | "search_replace" => ActivityCategory::EditingCode,
        "grep" | "glob" | "ls" | "list_dir" | "codebase_search" | "grep_search" | "file_search"
        | "search" | "find" | "semanticsearch" | "semantic_search" | "listdir" => {
            ActivityCategory::SearchingRepository
        }
        "webfetch" | "websearch" | "web_search" | "fetch" | "web_fetch" => {
            ActivityCategory::Research
        }
        "task" | "agent" | "spawn_agent" | "subagent" => ActivityCategory::Delegating,
        "bash" | "shell" | "powershell" | "run_terminal_cmd" | "terminal" | "exec_command"
        | "local_shell" | "exec" | "run_command" | "unified_exec" => command_text(input)
            .map(|c| classify_command(&c))
            .unwrap_or(ActivityCategory::RunningCommands),
        _ => ActivityCategory::Unknown,
    }
}

/// Extracts a plan snapshot from to-do style tool inputs (Claude `TodoWrite`,
/// Codex `update_plan`, Cursor to-do tools). Only counts are kept.
pub fn extract_plan(tool_name: &str, input: Option<&Value>) -> Option<PlanSnapshot> {
    let n = tool_name.to_ascii_lowercase();
    if !(n.contains("todo") || n.contains("plan")) {
        return None;
    }
    let input = input?;
    let items = ["todos", "plan", "items", "steps"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_array))?;
    let statuses: Vec<&str> = items
        .iter()
        .filter_map(|i| i.get("status").and_then(Value::as_str))
        .collect();
    if statuses.is_empty() || statuses.len() > 500 {
        return None;
    }
    let done = statuses
        .iter()
        .filter(|s| {
            matches!(
                s.to_ascii_lowercase().as_str(),
                "completed" | "done" | "complete"
            )
        })
        .count();
    Some(PlanSnapshot {
        total_steps: statuses.len() as u32,
        completed_steps: done as u32,
    })
}

/// Reads one change from task tools that edit a list a task at a time
/// (Claude `TaskCreate` and `TaskUpdate`): the task's id and its new status.
pub fn task_change(
    tool_name: &str,
    input: Option<&Value>,
    resp: Option<&Value>,
) -> Option<(String, String)> {
    let id = |v: &Value| {
        match v {
            Value::String(s) => safe_id(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
        .filter(|id| id.len() <= 64)
    };
    match tool_name {
        "TaskCreate" => {
            let resp = match resp? {
                Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
                v => v.clone(),
            };
            Some((id(resp.get("task")?.get("id")?)?, "pending".into()))
        }
        "TaskUpdate" => {
            let input = input?;
            let status = str_field(input, "status").and_then(safe_keyword)?;
            Some((id(input.get("taskId")?)?, status))
        }
        _ => None,
    }
}

/// Reads an exit code from common tool response shapes, if present.
pub fn exit_code(resp: Option<&Value>) -> Option<i32> {
    let resp = resp?;
    let obj = match resp {
        Value::String(s) => {
            return serde_json::from_str::<Value>(s)
                .ok()
                .and_then(|v| exit_code(Some(&v)))
        }
        v => v,
    };
    ["exit_code", "exitCode", "returncode", "return_code", "code"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_i64))
        .map(|c| c.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}

/// Walks up from `start` to the nearest directory containing `.git`.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    let mut depth = 0;
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        depth += 1;
        if depth > 64 {
            break;
        }
        cur = dir.parent();
    }
    None
}

/// Stable, salted repository identity. Raw paths never leave this function.
pub fn repository_ref(cwd: &str, ctx: &NormalizeContext) -> Option<RepositoryRef> {
    let cwd = cwd.trim();
    if cwd.is_empty() || cwd.len() > 4096 {
        return None;
    }
    let path = PathBuf::from(cwd);
    let root = find_repo_root(&path).unwrap_or(path);
    let mut key = root.to_string_lossy().replace('\\', "/");
    while key.ends_with('/') && key.len() > 1 {
        key.pop();
    }
    if cfg!(windows) {
        key = key.to_lowercase();
    }
    let mut h = Sha256::new();
    h.update(&ctx.salt);
    h.update(b"repo:");
    h.update(key.as_bytes());
    let id = format!("repo_{}", hex::encode(&h.finalize()[..10]));
    let display_name = if ctx.hook_config.store_repository_names {
        root.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| safe_text(n, 64))
    } else {
        None
    };
    Some(RepositoryRef { id, display_name })
}

/// Builds an envelope with a deterministic id.
#[allow(clippy::too_many_arguments)]
pub fn envelope(
    source: AgentSource,
    ctx: &NormalizeContext,
    raw: &[u8],
    discriminator: &str,
    session_id: String,
    turn_id: Option<String>,
    event_type: EventType,
    timestamp: Option<DateTime<Utc>>,
    mut payload: EventPayload,
) -> EventEnvelope {
    let ts = timestamp.unwrap_or(ctx.now);
    payload.launcher_id = ctx.launcher_id.as_deref().and_then(safe_id);
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: derive_event_id(
            source,
            &format!("{discriminator}/{}", event_type.as_str()),
            raw,
            ts,
        ),
        source,
        source_session_id: session_id,
        source_turn_id: turn_id,
        timestamp: ts,
        timestamp_origin: if timestamp.is_some() {
            TimestampOrigin::Source
        } else {
            TimestampOrigin::HookInvocation
        },
        event_type,
        payload,
    }
}

/// Derives classification features from the prompt and, if the user opted
/// in, a short title. The prompt itself is dropped when this returns.
/// Prompts the agent's harness submits on its own: a background command
/// reporting back, a message from another session. Claude Code wraps each in
/// a lowercase tag such as `<task-notification>`, which typed prompts do not
/// start with.
pub fn is_injected_prompt(text: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix('<') else {
        return false;
    };
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || *c == '-')
        .collect();
    name.len() >= 3
        && name.contains('-')
        && matches!(rest[name.len()..].chars().next(), Some('>' | ' ' | '\n'))
}

pub fn prompt_payload(prompt: Option<&str>, ctx: &NormalizeContext) -> EventPayload {
    use runmarq_estimator::classify;
    let mut p = EventPayload::default();
    if prompt.is_some_and(is_injected_prompt) {
        p.injected = Some(true);
        return p;
    }
    if let Some(text) = prompt.filter(|t| !t.trim().is_empty()) {
        let mut features = classify::classify(text, Some(&ctx.salt));
        if ctx.hook_config.prompt_sketches && !ctx.salt.is_empty() {
            features.sketch = Some(classify::sketch(text, &ctx.salt));
        }
        p.prompt = Some(features);
        if ctx.hook_config.store_task_titles {
            p.title = classify::derive_title(text);
        }
    }
    p
}

/// Effort as sent by agents: either a string or `{ "level": "high" }`.
pub fn effort_field(v: &Value) -> Option<String> {
    match v.get("effort")? {
        Value::String(s) => safe_keyword(s),
        Value::Object(o) => o
            .get("level")
            .and_then(Value::as_str)
            .and_then(safe_keyword),
        _ => None,
    }
}

/// Fields every adapter forwards when present: effort, permission mode, and
/// the transcript and working directory paths the collector reads locally.
pub fn session_context(v: &Value, payload: &mut EventPayload) {
    payload.effort = effort_field(v);
    payload.permission_mode = str_field(v, "permission_mode").and_then(safe_keyword);
    payload.transcript_path = str_field(v, "transcript_path")
        .filter(|p| p.len() <= 1024 && Path::new(p).is_absolute())
        .and_then(|p| safe_text(p, 1024));
    if payload.cwd.is_none() {
        let root = v
            .get("workspace_roots")
            .and_then(Value::as_array)
            .and_then(|r| r.first())
            .and_then(Value::as_str);
        payload.cwd = str_field(v, "cwd")
            .or(root)
            .filter(|p| p.len() <= 1024 && Path::new(p).is_absolute())
            .and_then(|p| safe_text(p, 1024));
    }
}

/// Adds the session context fields to every event produced from one payload.
pub fn with_session_context(v: &Value, events: &mut [(EventType, EventPayload)]) {
    let mut c = EventPayload::default();
    session_context(v, &mut c);
    for (_, p) in events.iter_mut() {
        p.effort = p.effort.take().or_else(|| c.effort.clone());
        p.permission_mode = p
            .permission_mode
            .take()
            .or_else(|| c.permission_mode.clone());
        p.transcript_path = p
            .transcript_path
            .take()
            .or_else(|| c.transcript_path.clone());
        p.cwd = p.cwd.take().or_else(|| c.cwd.clone());
    }
}

const SHELLS: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "dash",
    "fish",
    "pwsh",
    "powershell",
    "cmd",
];
const SHELL_FLAGS: &[&str] = &[
    "-c",
    "-lc",
    "-ic",
    "-command",
    "/c",
    "-noprofile",
    "-l",
    "-i",
    "/s",
];
const WRAPPERS: &[&str] = &[
    "sudo", "time", "env", "nice", "npx", "bunx", "pnpx", "dotenv",
];
const RUNNERS: &[&str] = &[
    "npm", "pnpm", "yarn", "bun", "deno", "uv", "poetry", "pipenv", "hatch",
];

fn program_name(token: &str) -> String {
    let base = token
        .trim_matches(|c| c == '"' || c == '\'')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(token);
    let lower = base.to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    lower
}

fn is_word(token: &str) -> bool {
    let t = token.trim_matches(|c| c == '"' || c == '\'');
    !t.is_empty()
        && t.len() <= 32
        && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || ":_-.".contains(c))
        && !t.contains("..")
        && !t.ends_with(".ts")
        && !t.ends_with(".js")
        && !t.ends_with(".py")
        && !t.ends_with(".rs")
}

/// Strips a shell wrapper such as `bash -lc '...'` and returns the inner command.
fn unwrap_shell(cmd: &str) -> String {
    let mut tokens = cmd.split_whitespace().peekable();
    let Some(first) = tokens.peek() else {
        return String::new();
    };
    if !SHELLS.contains(&program_name(first).as_str()) {
        return cmd.to_string();
    }
    tokens.next();
    let mut rest: Vec<&str> = Vec::new();
    let mut seen_flag = false;
    for t in tokens {
        if !seen_flag && SHELL_FLAGS.contains(&t.to_ascii_lowercase().as_str()) {
            seen_flag = true;
            continue;
        }
        rest.push(t);
    }
    if !seen_flag {
        return cmd.to_string();
    }
    rest.join(" ")
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
}

/// A short, normalized form of a shell command that identifies it across
/// runs: the program and up to two subcommand words, without flags, paths or
/// arguments. `npm test -- login` becomes `npm test`.
pub fn command_signature(cmd: &str) -> Option<String> {
    let inner = unwrap_shell(cmd.trim());
    let segments: Vec<&str> = inner
        .split(['&', '|', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let rank = |s: &str| match classify_command(s) {
        ActivityCategory::RunningTests => 0,
        ActivityCategory::Building => 1,
        ActivityCategory::InstallingDependencies => 2,
        _ => 3,
    };
    let segment = segments
        .iter()
        .filter(|s| !s.to_ascii_lowercase().starts_with("cd "))
        .min_by_key(|s| rank(s))
        .or_else(|| segments.first())?;

    let mut tokens = segment.split_whitespace().peekable();
    while let Some(t) = tokens.peek() {
        let is_env = t.contains('=')
            && t.split('=').next().is_some_and(|k| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
        if is_env || WRAPPERS.contains(&program_name(t).as_str()) {
            tokens.next();
        } else {
            break;
        }
    }
    let program = program_name(tokens.next()?);
    if program.is_empty() || !is_word(&program) {
        return None;
    }
    let mut parts = vec![program.clone()];
    let mut take = 2;
    while take > 0 {
        let Some(t) = tokens.next() else { break };
        if t == "--" || t.starts_with('-') && !(program == "python" || program == "python3") {
            break;
        }
        if (program == "python" || program == "python3") && t == "-m" {
            parts.push("-m".into());
            continue;
        }
        if !is_word(t) {
            break;
        }
        let word = t
            .trim_matches(|c| c == '"' || c == '\'')
            .to_ascii_lowercase();
        parts.push(word.clone());
        take -= 1;
        let more =
            RUNNERS.contains(&program.as_str()) && (word == "run" || word == "exec" || word == "x");
        if !more
            && !matches!(
                program.as_str(),
                "cargo"
                    | "go"
                    | "dotnet"
                    | "git"
                    | "make"
                    | "gradle"
                    | "mvn"
                    | "docker"
                    | "kubectl"
                    | "python"
                    | "python3"
            )
        {
            break;
        }
    }
    Some(parts.join(" "))
}

/// Salted identifier for a command signature; the readable form is not sent.
pub fn command_key(cmd: &str, ctx: &NormalizeContext) -> Option<String> {
    let sig = command_signature(cmd)?;
    let mut h = Sha256::new();
    h.update(&ctx.salt);
    h.update(b"cmd:");
    h.update(sig.as_bytes());
    Some(format!("cmd_{}", hex::encode(&h.finalize()[..10])))
}

fn line_count(v: Option<&Value>) -> u32 {
    v.and_then(Value::as_str)
        .map(|s| s.lines().count().min(100_000) as u32)
        .unwrap_or(0)
}

/// Approximate size of an edit tool call, from its input. Counts only lines.
pub fn edit_stats(tool_name: &str, input: Option<&Value>) -> Option<EditStats> {
    let input = input?;
    let n = tool_name.to_ascii_lowercase();
    let stats = match n.as_str() {
        "edit" | "str_replace" | "search_replace" | "strreplace" | "edit_file" => EditStats {
            lines_added: line_count(input.get("new_string").or_else(|| input.get("new_str"))),
            lines_removed: line_count(input.get("old_string").or_else(|| input.get("old_str"))),
        },
        "multiedit" => {
            let edits = input.get("edits").and_then(Value::as_array)?;
            edits.iter().fold(EditStats::default(), |acc, e| EditStats {
                lines_added: acc.lines_added + line_count(e.get("new_string")),
                lines_removed: acc.lines_removed + line_count(e.get("old_string")),
            })
        }
        "write" | "create_file" | "write_file" => EditStats {
            lines_added: line_count(input.get("content")),
            lines_removed: 0,
        },
        "notebookedit" => EditStats {
            lines_added: line_count(input.get("new_source")),
            lines_removed: 0,
        },
        "apply_patch" | "applypatch" => {
            let text = input
                .get("input")
                .or_else(|| input.get("patch"))
                .and_then(Value::as_str)
                .map(String::from)
                .or_else(|| command_text(Some(input)))?;
            let (mut a, mut r) = (0u32, 0u32);
            for l in text.lines().take(200_000) {
                if l.starts_with("+++") || l.starts_with("---") || l.starts_with("***") {
                    continue;
                }
                if l.starts_with('+') {
                    a += 1;
                } else if l.starts_with('-') {
                    r += 1;
                }
            }
            EditStats {
                lines_added: a,
                lines_removed: r,
            }
        }
        _ => return None,
    };
    (stats.lines_added > 0 || stats.lines_removed > 0).then_some(stats)
}

/// Command key and edit size for a tool call, filled in on the payload.
pub fn tool_details(
    tool_name: &str,
    input: Option<&Value>,
    ctx: &NormalizeContext,
    payload: &mut EventPayload,
) {
    let shell = matches!(
        payload.activity,
        Some(
            ActivityCategory::RunningTests
                | ActivityCategory::Building
                | ActivityCategory::InstallingDependencies
                | ActivityCategory::RunningCommands
                | ActivityCategory::Debugging
        )
    );
    if shell {
        if let Some(cmd) = command_text(input) {
            payload.command_key = command_key(&cmd, ctx);
        }
    }
    payload.edit = edit_stats(tool_name, input);
}

/// How a hook command line must be written for the shell that runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellStyle {
    /// Git Bash or PowerShell on Windows, POSIX shells elsewhere: forward
    /// slashes and no quotes work in all of them.
    PosixOrPowerShell,
    /// `cmd.exe /C` on Windows: backslashes, double-quoted path.
    Cmd,
    /// Unknown Windows shell (PowerShell or cmd): backslashes, no quotes.
    WindowsAny,
}

/// Formats the executable path for a hook command. On Windows, paths with
/// spaces are converted to their 8.3 short form so no quoting is needed.
pub fn hook_executable(exe: &Path, style: ShellStyle) -> String {
    let mut p = exe.to_path_buf();
    if cfg!(windows) && p.to_string_lossy().contains(' ') {
        if let Some(short) = short_path(&p) {
            p = short;
        }
    }
    let s = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    if cfg!(windows) {
        match style {
            ShellStyle::PosixOrPowerShell => s.replace('\\', "/"),
            ShellStyle::Cmd => format!("\"{s}\""),
            ShellStyle::WindowsAny => s,
        }
    } else if s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        s
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(windows)]
fn short_path(p: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: both buffers are valid for their stated lengths; the input is NUL-terminated.
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize >= buf.len() {
        return None;
    }
    let short = PathBuf::from(OsString::from_wide(&buf[..n as usize]));
    (!short.to_string_lossy().contains(' ')).then_some(short)
}

#[cfg(not(windows))]
fn short_path(_p: &Path) -> Option<PathBuf> {
    None
}

/// True when `command` is one of runmarq's hook commands for `source`,
/// including commands written under the pre-rename name.
pub fn is_runmarq_command(command: &str, source: AgentSource) -> bool {
    let c = command.to_ascii_lowercase();
    let current = runmarq_core::brand::brand().cli_name.to_ascii_lowercase();
    let exe_matches = c.contains(&current) || c.contains(runmarq_core::brand::LEGACY_CLI_NAME);
    exe_matches && c.contains(" emit") && c.contains(&format!("--source {}", source.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_from_the_agent_are_marked_and_not_classified() {
        let ctx = NormalizeContext {
            now: chrono::Utc::now(),
            hook_config: runmarq_core::api::HookConfig {
                store_task_titles: true,
                store_repository_names: true,
                prompt_sketches: true,
            },
            salt: b"salt".to_vec(),
            launcher_id: None,
        };
        let note = "<task-notification>
<task-id>b1</task-id>
<status>completed</status>
</task-notification>";
        let p = prompt_payload(Some(note), &ctx);
        assert_eq!(p.injected, Some(true));
        assert!(p.prompt.is_none() && p.title.is_none());
        let p = prompt_payload(Some("fix the failing login test"), &ctx);
        assert_eq!(p.injected, None);
        assert!(p.prompt.is_some());
        assert!(!is_injected_prompt("explain <task-notification> tags"));
        assert!(is_injected_prompt(
            "<cross-session-message from=\"a\">\nhello"
        ));
        assert!(!is_injected_prompt("<div> is not centered, fix it"));
        assert!(!is_injected_prompt("<T> generic bounds are wrong"));
    }
    use serde_json::json;

    #[test]
    fn commands() {
        assert_eq!(
            classify_command("cargo test --workspace"),
            ActivityCategory::RunningTests
        );
        assert_eq!(
            classify_command("pnpm add zod"),
            ActivityCategory::InstallingDependencies
        );
        assert_eq!(
            classify_command("npx tsc --noEmit"),
            ActivityCategory::Building
        );
        assert_eq!(
            classify_command("rg -n foo src"),
            ActivityCategory::SearchingRepository
        );
        assert_eq!(
            classify_command("/usr/bin/cat README.md"),
            ActivityCategory::ReadingFiles
        );
        assert_eq!(
            classify_command("git status"),
            ActivityCategory::RunningCommands
        );
    }

    #[test]
    fn tools() {
        assert_eq!(classify_tool("Edit", None), ActivityCategory::EditingCode);
        assert_eq!(
            classify_tool("Bash", Some(&json!({"command": "pytest -q"}))),
            ActivityCategory::RunningTests
        );
        assert_eq!(
            classify_tool(
                "shell",
                Some(&json!({"command": ["bash", "-lc", "npm test"]}))
            ),
            ActivityCategory::RunningTests
        );
        assert_eq!(
            classify_tool("mcp__github__get_issue", None),
            ActivityCategory::Research
        );
        assert_eq!(classify_tool("Task", None), ActivityCategory::Delegating);
        assert_eq!(
            classify_tool("SomethingNew", None),
            ActivityCategory::Unknown
        );
    }

    #[test]
    fn plans() {
        let input = json!({"todos": [{"content": "a", "status": "completed"}, {"content": "b", "status": "in_progress"}, {"content": "c", "status": "pending"}]});
        assert_eq!(
            extract_plan("TodoWrite", Some(&input)),
            Some(PlanSnapshot {
                total_steps: 3,
                completed_steps: 1
            })
        );
        let codex = json!({"plan": [{"step": "x", "status": "completed"}, {"step": "y", "status": "completed"}]});
        assert_eq!(
            extract_plan("update_plan", Some(&codex)),
            Some(PlanSnapshot {
                total_steps: 2,
                completed_steps: 2
            })
        );
        assert_eq!(extract_plan("Bash", Some(&input)), None);
    }

    #[test]
    fn ids_are_sanitized() {
        assert_eq!(safe_id("abc-123").as_deref(), Some("abc-123"));
        let hashed = safe_id("a b; rm -rf /").unwrap();
        assert!(hashed.starts_with("h_") && hashed.chars().all(id_char));
        assert_eq!(safe_id("  "), None);
        assert_eq!(
            safe_tool_name("Bash$(whoami)").as_deref(),
            Some("Bashwhoami")
        );
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(Some(&json!({"exit_code": 2}))), Some(2));
        assert_eq!(exit_code(Some(&json!("{\"exitCode\": 1}"))), Some(1));
        assert_eq!(exit_code(Some(&json!({"stdout": "x"}))), None);
    }

    #[test]
    fn repository_identity_hides_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let sub = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        let ctx = NormalizeContext {
            now: Utc::now(),
            hook_config: runmarq_core::api::HookConfig {
                store_task_titles: false,
                store_repository_names: true,
                prompt_sketches: false,
            },
            salt: b"s".to_vec(),
            launcher_id: None,
        };
        let a = repository_ref(&sub.to_string_lossy(), &ctx).unwrap();
        let b = repository_ref(&dir.path().to_string_lossy(), &ctx).unwrap();
        assert_eq!(a.id, b.id, "subdirectories map to the repository root");
        assert!(!a.id.contains(&*dir.path().to_string_lossy()));
        assert_eq!(
            a.display_name.as_deref(),
            dir.path().file_name().and_then(|n| n.to_str())
        );
        let mut private = ctx.clone();
        private.hook_config.store_repository_names = false;
        assert_eq!(
            repository_ref(&sub.to_string_lossy(), &private)
                .unwrap()
                .display_name,
            None
        );
    }

    #[test]
    fn command_signatures_keep_only_the_program_and_subcommands() {
        let sig = |c: &str| command_signature(c);
        assert_eq!(sig("npm test -- --grep login").as_deref(), Some("npm test"));
        assert_eq!(
            sig("cargo test -p runmarq-core --lib").as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            sig("npm run test:unit -- --watch=false").as_deref(),
            Some("npm run test:unit")
        );
        assert_eq!(
            sig("python -m pytest tests/test_auth.py -k login").as_deref(),
            Some("python -m pytest")
        );
        assert_eq!(
            sig("bash -lc 'cd app && CI=1 npx vitest run src/a.test.ts'").as_deref(),
            Some("vitest run")
        );
        assert_eq!(
            sig("C:\\tools\\go.exe test ./...").as_deref(),
            Some("go test")
        );
        assert_eq!(
            sig("./scripts/check.sh --fast").as_deref(),
            Some("check.sh")
        );
        assert_eq!(sig("--version").as_deref(), None);
        assert_eq!(sig("   ").as_deref(), None);
    }

    #[test]
    fn command_keys_are_salted() {
        let mut a = ctx_with_salt(b"one");
        let k1 = command_key("npm test", &a).unwrap();
        assert_eq!(command_key("npm test -- --grep x", &a).unwrap(), k1);
        assert!(k1.starts_with("cmd_") && !k1.contains("npm"));
        a.salt = b"two".to_vec();
        assert_ne!(command_key("npm test", &a).unwrap(), k1);
    }

    fn ctx_with_salt(salt: &[u8]) -> NormalizeContext {
        NormalizeContext {
            now: Utc::now(),
            hook_config: runmarq_core::api::HookConfig::default(),
            salt: salt.to_vec(),
            launcher_id: None,
        }
    }

    #[test]
    fn edit_sizes_count_lines_only() {
        let e = edit_stats(
            "Edit",
            Some(&json!({"old_string": "a\nb", "new_string": "a\nb\nc"})),
        )
        .unwrap();
        assert_eq!((e.lines_added, e.lines_removed), (3, 2));
        let m = edit_stats(
            "MultiEdit",
            Some(&json!({"edits": [{"old_string": "x", "new_string": "y\nz"}, {"old_string": "p", "new_string": "q"}]})),
        )
        .unwrap();
        assert_eq!((m.lines_added, m.lines_removed), (3, 2));
        let patch = "*** Begin Patch\n*** Update File: a.rs\n@@\n-old\n+new\n+more\n*** End Patch";
        let p = edit_stats("apply_patch", Some(&json!({"input": patch}))).unwrap();
        assert_eq!((p.lines_added, p.lines_removed), (2, 1));
        assert!(edit_stats("Read", Some(&json!({"file_path": "/x"}))).is_none());
        assert!(edit_stats("Write", Some(&json!({"content": ""}))).is_none());
    }

    #[test]
    fn session_context_reads_effort_and_paths() {
        let abs = if cfg!(windows) {
            "C:\\work\\t.jsonl"
        } else {
            "/work/t.jsonl"
        };
        let v = json!({
            "effort": {"level": "high"},
            "permission_mode": "acceptEdits",
            "transcript_path": abs,
            "cwd": "relative/dir",
        });
        let mut p = EventPayload::default();
        session_context(&v, &mut p);
        assert_eq!(p.effort.as_deref(), Some("high"));
        assert_eq!(p.permission_mode.as_deref(), Some("acceptedits"));
        assert_eq!(p.transcript_path.as_deref(), Some(abs));
        assert_eq!(p.cwd, None, "relative paths are ignored");
        let mut q = EventPayload::default();
        session_context(&json!({"effort": "low"}), &mut q);
        assert_eq!(q.effort.as_deref(), Some("low"));
    }

    #[test]
    fn runmarq_commands_are_recognized() {
        assert!(is_runmarq_command(
            "C:/x/runmarq.exe emit --source claude_code",
            AgentSource::ClaudeCode
        ));
        assert!(!is_runmarq_command(
            "C:/x/runmarq.exe emit --source cursor",
            AgentSource::ClaudeCode
        ));
        assert!(!is_runmarq_command(
            "python3 ~/.claude/hooks/mine.py",
            AgentSource::ClaudeCode
        ));
        assert!(is_runmarq_command(
            "C:/Users/me/AppData/Local/Runmark/bin/runmark.exe emit --source claude_code",
            AgentSource::ClaudeCode
        ));
    }
}
