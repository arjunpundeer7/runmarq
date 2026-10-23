# Decisions

Short records of choices that were not obvious. Newest last.

### 1. rusqlite over SQLx
One local single-user database, accessed from one collector process behind a mutex. rusqlite (with bundled SQLite) gives synchronous, parameterized queries without a pool or compile-time database. Queries run on Tokio's blocking pool.

### 2. Normalize in the hook process
`runmarq emit` turns vendor payloads into minimized events before anything is sent or spooled. Prompt text therefore never crosses a process boundary or touches disk, and the spool contains only safe data. Cost: adapter changes require the hook binary to be updated, which the stable copy in `<data>/bin` handles.

### 3. The collector is its own process
The desktop app spawns the collector detached instead of embedding it, so a UI crash or restart cannot interrupt collection. A file lock guarantees one instance; the CLI and app share it.

### 4. Hooks call a stable copy of the CLI
Setup copies `runmarq` to `<data>/bin/`, and hooks reference that path, so moving or updating the app, or rebuilding during development, never breaks agent configuration. The copy is refreshed when its hash differs. On Windows, a running copy is renamed aside first.

### 5. Shell quoting per vendor
Each vendor runs hook commands through a different shell, so each gets its own command form:
- **Claude Code** runs hooks via Git Bash (or PowerShell without it): forward-slash path, no quotes.
- **Codex** runs `cmd.exe /C` on Windows, as read from `codex-rs/hooks/src/engine/command_runner.rs`: a quoted backslash path.
- **Cursor**'s Windows shell is undocumented: a backslash path without quotes, which works in both PowerShell and cmd.

Paths containing spaces use the Windows 8.3 short form.

### 6. Cursor: after-the-fact hooks only
Cursor's permission hooks block on malformed output, and an explicit "allow" may bypass the user's approval prompt. Registering them would let an observer change agent behaviour. runmarq therefore forgoes tool-start and approval events for Cursor and says so in its capability manifest.

### 7. Claude Code hooks are synchronous
Claude Code documents `async: true` for command hooks, but it is not clear that 2.1.143 accepts it, and an unknown key could invalidate settings. The hook path costs about 1 ms of work plus process start-up (measured 70–90 ms end to end from Git Bash on Windows), so synchronous hooks are acceptable.

### 8. Interrupted runs are censored, failed runs half-weighted
A cancelled or interrupted run only tells the model that the work lasted at least as long as it did, so it enters through one EM-style imputation step at half weight. Failed runs did end, but for different reasons, so they also count at half weight. Neither is treated as an ordinary successful duration.

### 9. Disjoint cohort sets
Shrinking nested cohorts (global → category → agent+category → repository) sequentially re-uses the same runs at every level and makes a single run look conclusive. History is instead partitioned into disjoint sets from least to most similar, each shrunk toward the estimate before it.

### 10. Website demo from the real engine
`crates/collector/examples/demo_run.rs` replays a scripted run through the real engine and writes `apps/web/content/demo-run.json`. The website demonstration has no hand-written numbers.

### 11. Validation decides model weights
Each model's weight comes from how well it forecast the user's own recent runs out of sample, not from fixed heuristics. A model that does not help gets weight zero and is reported as not in use. The learned model is used only after it has been validated on at least ten unseen runs.

### 12. Conformal calibration over parametric intervals
Parametric intervals are only as good as their distributional assumptions, which agent runs do not respect. Split-conformal calibration on out-of-sample forecasts gives a coverage guarantee under exchangeability with no such assumption. Scores are kept per forecast-age bucket because early and late forecasts err differently. The guarantee weakens if working habits change quickly; refitting every few runs limits that.

### 13. Numbers from transcripts, read by the collector
Token counts and context size predict how much work remains, and agents already write them to local session files. The collector, not the hook, reads them: it lives for the whole run and can read incrementally. Only `.jsonl` files are opened, only numeric fields are parsed, and the path is kept in memory only.

### 14. TypeScript 5.9
TypeScript 7 (the native compiler) is current, but typescript-eslint and the Next.js type plugin target 5.x. 5.9.3 is pinned until they support 7.
