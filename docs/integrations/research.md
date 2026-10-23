# Integration research and verification status

What each adapter relies on, where it came from, and what has and has not been verified. Re-check this file against the vendor's documentation whenever a vendor releases a major version.

## Claude Code (tested against 2.1.143)

- **Source:** the hooks reference at code.claude.com/docs/en/hooks and the settings reference at code.claude.com/docs/en/settings.
- **Event names:** all 13 registered events were found as strings in the installed 2.1.143 binary.
- **Payload fields:** taken from the documentation. Every event carries `session_id`, `cwd` and `hook_event_name`. Event-specific fields: `UserPromptSubmit` has `prompt`; the tool events have `tool_name`, `tool_input`, `tool_use_id`, `tool_response` and `duration_ms`; `PostToolUseFailure` has `is_interrupt`; `Notification` has `notification_type`.
- **Observer safety:** plain stdout is added to the conversation for `UserPromptSubmit` and `SessionStart`, and exit 2 blocks. runmarq prints nothing and always exits 0.
- **Interrupts:** the docs state that `Stop` does not run on a user interrupt, so interrupts are inferred.
- **Cursor overlap:** Cursor executes Claude Code settings hooks, and its payloads include `cursor_version`; those payloads are ignored.
- **Not yet verified:**
  - a live end-to-end session with the hooks installed;
  - the exact shape of `tool_response` for Bash (exit codes are read only if present);
  - whether a non-zero Bash exit arrives as `PostToolUseFailure` or as `PostToolUse`. Both paths are handled.

## Codex (tested against 0.130.0)

- **Hooks are stable:** `codex features list` reports `hooks stable true`.
- **Event names:** 8 are defined in the binary's app-server enum and embedded input schemas: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact` and `Stop`.
- **Payload fields:** taken from the embedded `*.command.input` schemas. `turn_id` is present on every event except `SessionStart`.
- **Trust:** hooks must be trusted with `/hooks` before they run, and the trust is tied to a hash of the hook (`trusted_hash` exists in the binary).
- **Windows shell:** `cmd.exe /C` with the environment cleared and then restored from the session snapshot, according to `codex-rs/hooks/src/engine/command_runner.rs`.
- **Not yet verified:**
  - a live session. Trust requires the interactive TUI.
  - that `RUNMARQ_LAUNCHER_ID` and `RUNMARQ_SUPPRESS_HOOKS` survive Codex's environment replay. If they do not, wrapper-launched runs still close through hooks, but process-exit closing and exec-mode deduplication would not work.
  - the `codex exec --json` event names. They come from the Codex source and were confirmed as strings in the binary, but not observed in a live stream.
- **Local note:** on the development machine, `~/.codex/config.toml` failed to parse under 0.130 (`service_tier = "default"` is no longer a valid value). runmarq does not touch `config.toml`, but Codex itself will report that error.

## Cursor (tested against 3.23.12)

- **Source:** the hooks documentation at cursor.com/docs/hooks.
- **Fields:** common fields are `conversation_id`, `generation_id`, `model`, `hook_event_name`, `cursor_version` and `workspace_roots`. `stop` has `status` (`completed`, `aborted` or `error`).
- **Permission hooks are not used:** invalid output from them blocks the action, and whether an explicit "allow" bypasses approval is undocumented.
- **Acknowledgements:** documented pass-through outputs are `{"continue": true}` for `beforeSubmitPrompt` and `{}` for observer hooks.
- **Windows shell:** undocumented. Community reports suggest PowerShell.
- **Not yet verified:** a live Cursor session, the Windows shell behaviour, and the field name for per-turn token usage (mentioned in the Cursor changelog but absent from the reference).

## Verified live

- **Claude Code 2.1.143, 2026-10-21.** Real sessions running the installed hooks produced valid normalized events. Observed: `session_started`, `prompt_submitted` (classified, with no prompt text stored), `tool_started`/`tool_completed` for reading, searching, running commands and running tests, and `run_completed`. The events were spooled because no collector was running, and replay is covered by the collector tests.
- **Windows PowerShell stdin encoding.** Piping JSON to a native program from Windows PowerShell 5.1 can add a UTF-8 BOM or send UTF-16. `runmarq emit` normalizes both before parsing (`decode_stdin`). This matters for any hook runner that uses PowerShell, which Cursor appears to on Windows.

## Capturing real fixtures

Set `RUNMARQ_CAPTURE_DIR` for a session and `runmarq emit` writes each raw payload to that folder. Captures contain prompts and tool input, so redact them before adding them to `fixtures/`.
