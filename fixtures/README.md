# Event fixtures

Each `.jsonl` file contains raw vendor hook payloads, one per line, in the order an agent would deliver them. The adapter and collector tests replay these files.

| Path | Where the format comes from |
|------|-----------------------------|
| `claude/` | Field lists from the Claude Code hooks reference (code.claude.com/docs/en/hooks). Event names were checked against the installed Claude Code 2.1.143 binary. |
| `cursor/` | Field lists from the Cursor hooks reference (cursor.com/docs/hooks). |
| `codex/hooks-run.jsonl` | Input schemas embedded in the Codex 0.130.0 binary (`*.command.input`). |
| `codex/exec-stream.jsonl` | `codex exec --json` event types from the Codex source (`codex-rs/exec/src/exec_events.rs`). |

These fixtures are **contract-derived**. They were written from the documented and embedded formats, not captured from live sessions. Some fields that the documentation does not describe in detail, such as the exact shape of `tool_response`, are illustrative.

To add captured fixtures, set `RUNMARQ_CAPTURE_DIR` before an agent session. `runmarq emit` then writes each raw payload it receives to that directory. Captures contain prompts and tool input, so review them and remove private content before committing them.
