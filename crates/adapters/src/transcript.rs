//! Parses agent session transcripts into numbers. Message text is never read
//! into the returned values; only token counts, timestamps, model and effort.

use chrono::{DateTime, Utc};
use runmarq_core::event::AgentSource;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    /// Claude Code: one JSON object per line, assistant messages carry `message.usage`.
    Claude,
    /// Codex rollout files: `event_msg/token_count` carries running totals.
    Codex,
}

impl TranscriptFormat {
    pub fn for_source(source: AgentSource) -> Option<TranscriptFormat> {
        match source {
            AgentSource::ClaudeCode => Some(TranscriptFormat::Claude),
            AgentSource::Codex => Some(TranscriptFormat::Codex),
            AgentSource::Cursor => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// One model response. Claude splits a response across several lines that
    /// share `id`; consumers keep the largest counts per id.
    Message {
        id: Option<String>,
        at: Option<DateTime<Utc>>,
        output_tokens: u64,
        thinking_tokens: u64,
        context_tokens: u64,
        model: Option<String>,
        effort: Option<String>,
        sidechain: bool,
    },
    /// Running totals for the whole session (Codex).
    Totals {
        at: Option<DateTime<Utc>>,
        output_tokens: u64,
        reasoning_tokens: u64,
        context_tokens: u64,
        context_window: Option<u64>,
    },
    /// Turn settings (Codex).
    Context {
        at: Option<DateTime<Utc>>,
        model: Option<String>,
        effort: Option<String>,
    },
    Compaction {
        at: Option<DateTime<Utc>>,
    },
}

impl Entry {
    pub fn at(&self) -> Option<DateTime<Utc>> {
        match self {
            Entry::Message { at, .. }
            | Entry::Totals { at, .. }
            | Entry::Context { at, .. }
            | Entry::Compaction { at } => *at,
        }
    }
}

fn num(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return 0,
        }
    }
    cur.as_u64().unwrap_or(0)
}

fn short(v: Option<&Value>, max: usize) -> Option<String> {
    let s = v?.as_str()?.trim();
    if s.is_empty() || s.len() > max || s.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(s.to_string())
}

fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    v.get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn effort_value(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(_) => short(v, 24),
        Value::Object(o) => short(o.get("level"), 24),
        _ => None,
    }
}

pub fn parse_line(format: TranscriptFormat, line: &str) -> Option<Entry> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    match format {
        TranscriptFormat::Claude => parse_claude(&v),
        TranscriptFormat::Codex => parse_codex(&v),
    }
}

fn parse_claude(v: &Value) -> Option<Entry> {
    match v.get("type")?.as_str()? {
        "assistant" => {
            let msg = v.get("message")?;
            let usage = msg.get("usage")?;
            let model = short(msg.get("model"), 96).filter(|m| !m.starts_with('<'));
            Some(Entry::Message {
                id: short(msg.get("id"), 128),
                at: timestamp(v),
                output_tokens: num(usage, &["output_tokens"]),
                thinking_tokens: num(usage, &["output_tokens_details", "thinking_tokens"]),
                context_tokens: num(usage, &["input_tokens"])
                    + num(usage, &["cache_read_input_tokens"])
                    + num(usage, &["cache_creation_input_tokens"]),
                model,
                effort: effort_value(v.get("effort")),
                sidechain: v
                    .get("isSidechain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        "system" if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => {
            Some(Entry::Compaction { at: timestamp(v) })
        }
        _ => None,
    }
}

fn parse_codex(v: &Value) -> Option<Entry> {
    let payload = v.get("payload");
    match v.get("type")?.as_str()? {
        "event_msg" => {
            let p = payload?;
            if p.get("type")?.as_str()? != "token_count" {
                return None;
            }
            let info = p.get("info").filter(|i| !i.is_null())?;
            Some(Entry::Totals {
                at: timestamp(v),
                output_tokens: num(info, &["total_token_usage", "output_tokens"]),
                reasoning_tokens: num(info, &["total_token_usage", "reasoning_output_tokens"]),
                context_tokens: num(info, &["last_token_usage", "input_tokens"]),
                context_window: info.get("model_context_window").and_then(Value::as_u64),
            })
        }
        "turn_context" => {
            let p = payload?;
            let effort = effort_value(p.get("effort")).or_else(|| {
                effort_value(p.pointer("/collaboration_mode/settings/reasoning_effort"))
            });
            Some(Entry::Context {
                at: timestamp(v),
                model: short(p.get("model"), 96),
                effort,
            })
        }
        "compacted" => Some(Entry::Compaction { at: timestamp(v) }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_assistant_usage() {
        let line = r#"{"type":"assistant","timestamp":"2026-10-21T10:00:05.000Z","isSidechain":false,"effort":"high",
            "message":{"id":"msg_1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"secret"}],
            "usage":{"input_tokens":12,"cache_read_input_tokens":40000,"cache_creation_input_tokens":800,"output_tokens":350,
            "output_tokens_details":{"thinking_tokens":120}}}}"#;
        let e = parse_line(TranscriptFormat::Claude, line).unwrap();
        assert_eq!(
            e,
            Entry::Message {
                id: Some("msg_1".into()),
                at: Some(
                    DateTime::parse_from_rfc3339("2026-10-21T10:00:05Z")
                        .unwrap()
                        .with_timezone(&Utc)
                ),
                output_tokens: 350,
                thinking_tokens: 120,
                context_tokens: 40812,
                model: Some("claude-opus-5-5".into()),
                effort: Some("high".into()),
                sidechain: false,
            }
        );
        assert!(!format!("{e:?}").contains("secret"));
    }

    #[test]
    fn claude_compaction_and_ignored_lines() {
        let c =
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-10-21T10:00:00Z"}"#;
        assert!(matches!(
            parse_line(TranscriptFormat::Claude, c),
            Some(Entry::Compaction { .. })
        ));
        for l in [
            r#"{"type":"user","message":{"content":"hi"}}"#,
            "not json",
            "",
            r#"{"type":"assistant"}"#,
        ] {
            assert_eq!(parse_line(TranscriptFormat::Claude, l), None);
        }
        let synthetic =
            r#"{"type":"assistant","message":{"model":"<synthetic>","usage":{"output_tokens":0}}}"#;
        assert!(matches!(
            parse_line(TranscriptFormat::Claude, synthetic),
            Some(Entry::Message { model: None, .. })
        ));
    }

    #[test]
    fn codex_totals_context_and_compaction() {
        let t = r#"{"timestamp":"2026-10-21T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{
            "total_token_usage":{"input_tokens":23727,"cached_input_tokens":13184,"output_tokens":212,"reasoning_output_tokens":64,"total_tokens":23939},
            "last_token_usage":{"input_tokens":9000,"output_tokens":40},"model_context_window":258400}}}"#;
        assert_eq!(
            parse_line(TranscriptFormat::Codex, t),
            Some(Entry::Totals {
                at: Some(
                    DateTime::parse_from_rfc3339("2026-10-21T10:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc)
                ),
                output_tokens: 212,
                reasoning_tokens: 64,
                context_tokens: 9000,
                context_window: Some(258400),
            })
        );
        let no_info = r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#;
        assert_eq!(parse_line(TranscriptFormat::Codex, no_info), None);
        let ctx = r#"{"type":"turn_context","payload":{"model":"gpt-5.1-codex","collaboration_mode":{"settings":{"reasoning_effort":"medium"}}}}"#;
        assert_eq!(
            parse_line(TranscriptFormat::Codex, ctx),
            Some(Entry::Context {
                at: None,
                model: Some("gpt-5.1-codex".into()),
                effort: Some("medium".into())
            })
        );
        assert!(matches!(
            parse_line(
                TranscriptFormat::Codex,
                r#"{"type":"compacted","payload":{"message":"x"}}"#
            ),
            Some(Entry::Compaction { .. })
        ));
    }

    #[test]
    fn effort_object_form() {
        let line = r#"{"type":"assistant","effort":{"level":"xhigh"},"message":{"usage":{"output_tokens":1}}}"#;
        assert!(matches!(
            parse_line(TranscriptFormat::Claude, line),
            Some(Entry::Message { effort: Some(e), .. }) if e == "xhigh"
        ));
    }
}
