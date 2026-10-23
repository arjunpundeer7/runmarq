//! Editing for the matcher-group hook schema shared by Claude Code and Codex:
//!
//! ```json
//! { "hooks": { "PreToolUse": [ { "matcher": "*", "hooks": [ { "type": "command", "command": "...", "timeout": 10 } ] } ] } }
//! ```
//!
//! runmarq owns only hook handlers whose command is a runmarq `emit` command
//! for the given source. Everything else is left exactly as found.

use crate::common::is_runmarq_command;
use runmarq_core::event::AgentSource;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy)]
pub struct HookSpec {
    pub event: &'static str,
    /// Whether to write `"matcher": "*"` (only for events that match on tool names).
    pub tool_matcher: bool,
}

fn is_ours(handler: &Value, source: AgentSource) -> bool {
    handler
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| is_runmarq_command(c, source))
}

/// Removes runmarq handlers from every event. Groups and event arrays that
/// become empty *because of the removal* are dropped. Returns handlers removed.
pub fn strip(hooks: &mut Map<String, Value>, source: AgentSource) -> usize {
    let mut removed = 0;
    let mut empty_events = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let before_groups = groups.len();
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|h| !is_ours(h, source));
            let n = before - handlers.len();
            removed += n;
            !(n > 0 && handlers.is_empty())
        });
        if groups.is_empty() && before_groups > 0 {
            empty_events.push(event.clone());
        }
    }
    for e in empty_events {
        hooks.remove(&e);
    }
    removed
}

/// Installs one runmarq handler per event, replacing any previous runmarq
/// handlers (so a moved executable is picked up). Idempotent.
///
/// Returns events that were skipped because the user's existing value for
/// that event is not an array (invalid for the vendor too). Those values are
/// left untouched rather than overwritten.
pub fn install(
    hooks: &mut Map<String, Value>,
    source: AgentSource,
    specs: &[HookSpec],
    command: &str,
    timeout: u64,
) -> Vec<&'static str> {
    strip(hooks, source);
    let mut skipped = Vec::new();
    for spec in specs {
        let handler = json!({ "type": "command", "command": command, "timeout": timeout });
        let mut group = Map::new();
        if spec.tool_matcher {
            group.insert("matcher".into(), json!("*"));
        }
        group.insert("hooks".into(), json!([handler]));
        let entry = hooks
            .entry(spec.event.to_string())
            .or_insert_with(|| json!([]));
        match entry.as_array_mut() {
            Some(groups) => groups.push(Value::Object(group)),
            None => skipped.push(spec.event),
        }
    }
    skipped
}

/// Events (from `specs`) that have a runmarq handler, and the commands used.
pub fn installed(
    hooks: &Map<String, Value>,
    source: AgentSource,
    specs: &[HookSpec],
) -> Vec<(&'static str, Option<String>)> {
    specs
        .iter()
        .map(|spec| {
            let cmd = hooks
                .get(spec.event)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|g| g.get("hooks").and_then(Value::as_array))
                .flatten()
                .find(|h| is_ours(h, source))
                .and_then(|h| h.get("command").and_then(Value::as_str).map(String::from));
            (spec.event, cmd)
        })
        .collect()
}

pub fn preview(specs: &[HookSpec], command: &str, timeout: u64) -> Value {
    let mut hooks = Map::new();
    install(&mut hooks, AgentSource::ClaudeCode, specs, command, timeout);
    json!({ "hooks": hooks })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPECS: &[HookSpec] = &[
        HookSpec {
            event: "PreToolUse",
            tool_matcher: true,
        },
        HookSpec {
            event: "Stop",
            tool_matcher: false,
        },
    ];

    #[test]
    fn install_preserves_user_hooks_and_is_idempotent() {
        let mut hooks: Map<String, Value> = serde_json::from_value(json!({
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./my-guard.sh"}]}],
            "Notification": [{"hooks": [{"type": "command", "command": "notify-send hi"}]}]
        }))
        .unwrap();
        let cmd = "/opt/runmarq emit --source claude_code";
        install(&mut hooks, AgentSource::ClaudeCode, SPECS, cmd, 10);
        let once = hooks.clone();
        install(&mut hooks, AgentSource::ClaudeCode, SPECS, cmd, 10);
        assert_eq!(hooks, once, "idempotent");
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(
            hooks["PreToolUse"][0]["hooks"][0]["command"],
            "./my-guard.sh"
        );
        assert_eq!(
            hooks["Notification"][0]["hooks"][0]["command"],
            "notify-send hi"
        );
        assert!(hooks["Stop"][0].get("matcher").is_none());

        let found = installed(&hooks, AgentSource::ClaudeCode, SPECS);
        assert!(found.iter().all(|(_, c)| c.as_deref() == Some(cmd)));

        assert_eq!(strip(&mut hooks, AgentSource::ClaudeCode), 2);
        assert!(
            hooks.get("Stop").is_none(),
            "event created by runmarq removed"
        );
        assert_eq!(
            hooks["PreToolUse"].as_array().unwrap().len(),
            1,
            "user group kept"
        );
        assert!(hooks.get("Notification").is_some());
    }

    #[test]
    fn shared_group_keeps_other_handlers() {
        let cmd = "runmarq emit --source claude_code";
        let mut hooks: Map<String, Value> = serde_json::from_value(json!({
            "Stop": [{"hooks": [{"type": "command", "command": "say done"}, {"type": "command", "command": cmd}]}]
        }))
        .unwrap();
        assert_eq!(strip(&mut hooks, AgentSource::ClaudeCode), 1);
        assert_eq!(hooks["Stop"][0]["hooks"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn moved_executable_is_replaced() {
        let mut hooks = Map::new();
        install(
            &mut hooks,
            AgentSource::Codex,
            SPECS,
            "/old/runmarq emit --source codex",
            10,
        );
        install(
            &mut hooks,
            AgentSource::Codex,
            SPECS,
            "/new/runmarq emit --source codex",
            10,
        );
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(
            hooks["Stop"][0]["hooks"][0]["command"],
            "/new/runmarq emit --source codex"
        );
    }
}
