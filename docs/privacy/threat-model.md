# Threat model

| Threat | Control | Where |
|---|---|---|
| Malicious or malformed hook input | stdin bounded to 1 MB; JSON parse failures ignored; every vendor string bounded and character-filtered; ids that are not clean are hashed; `deny_unknown_fields` on envelopes, unknown payload fields dropped, and `validate()` on every field | `crates/cli/src/emit.rs`, `crates/adapters/src/common.rs`, `crates/core/src/event.rs` |
| Oversized requests | 512 KB body limit, Content-Length pre-check, 500 events per request, 16 KB envelope bound | `crates/collector/src/server.rs` |
| Other local users | Token in a user-only data directory: `0700`/`0600` on Unix, inherited `%LOCALAPPDATA%` ACL on Windows. Constant-time token comparison | `crates/collector/src/lifecycle.rs` |
| Browser pages attacking localhost | Requests with any `Origin` header are rejected, so no CORS. A token is required. JSON-only POST bodies force a CORS preflight | `server.rs` `guard` |
| DNS rebinding | The `Host` header must be `127.0.0.1:<port>` or `localhost:<port>` | `server.rs` `guard` |
| Token exposure to the webview | Only the Tauri native side holds the token; the webview calls typed commands | `apps/desktop/src-tauri/src/commands.rs` |
| Symlink attacks on spool and discovery files | `create_new`, plus `O_NOFOLLOW` on Unix; `symlink_metadata` checks when reading; non-regular files skipped | `lifecycle.rs`, `spool.rs` |
| Unbounded disk use | Spool limited to 5 MB and 2000 files; log rotation; optional history retention | `spool.rs`, `emit.rs`, `engine.rs` |
| Configuration corruption | Unparseable files are refused; backup before every change; atomic temp-file-and-rename writes; key order preserved; only runmarq-owned entries removed | `crates/adapters/src/config.rs`, `grouped.rs`, `cursor.rs` |
| Hooks changing agent behaviour | Silent stdout (Claude Code, Codex), documented pass-through acknowledgements (Cursor), always exit 0, no permission hooks in Cursor | adapters |
| Command execution from event data | No event field is ever executed. Detection runs fixed arguments (`--version`) on resolved executables only. The working-tree probe runs `git` with fixed arguments in the reported directory, with `core.fsmonitor`, external diff and textconv disabled, optional locks off, no terminal prompts, a 3-second timeout and bounded output | `crates/adapters/src/detect.rs`, `crates/collector/src/probe.rs` |
| Reading files named in event data | Only `.jsonl` transcript paths are opened, read-only and incrementally (at most 16 MB on first read, 8 MB per poll); lines are parsed for numbers only and never stored or logged; paths are kept in memory, never in the database | `crates/collector/src/probe.rs`, `engine.rs` |
| Linking prompts across installs | Prompt word vectors and command keys are derived with the per-install salt, so they cannot be compared between machines | `crates/estimator/src/classify.rs`, `crates/adapters/src/common.rs` |
| Secrets in captured text | Titles are opt-in and redacted: secret-looking tokens and `key=value` pairs are replaced. Commands are never stored | `crates/estimator/src/classify.rs` |
| Prompt injection via captured text | Captured text is never interpreted, executed or sent to a model. Transcripts are parsed as JSON for numeric fields only | by design |
| Cross-session leakage | Sessions keyed by (agent, vendor id); runs keyed by session and turn | `engine.rs` |
| CSV formula injection | Fields are quoted, and values starting with `= + - @` are prefixed with `'` | `server.rs` `csv_field` |
| Website form abuse | Server-side validation, honeypot, per-IP rate limit, required consent, HTTPS-only backend, no fake success | `apps/web/app/api/early-access/route.ts` |
| Insecure updates | No auto-update yet. Release artifacts are published with SHA-256 checksums, and signing status is shown truthfully | `scripts/release-manifest.mjs`, download page |

## Known gaps

- **Windows ACLs:** the default ACLs are relied on rather than hardened explicitly with an ACL API.
- **Rate limiting:** the website's limit is per instance; deployments with several instances should also rate-limit at the edge.
- **Signing:** releases are signed only when signing secrets are configured. See `docs/development/release.md`.
