# Data inventory

Everything runmarq writes, and where.

| Location (in the data directory) | Contents | Sensitive? |
|---|---|---|
| `runmarq.db` | Runs (timing, state, category, counts, effort level, permission mode, optional title, run signals), events (type, time, tool name, activity, duration, minimized payload), run signal snapshots, command timings, forecasts (including uncalibrated quantiles), sessions, repositories (salted id, optional folder name, tracked file count), preferences, notification log, installation records | Optional titles and folder names, and prompt word vectors. All are user-controlled, and turning them off erases them. |
| `collector.json` | Collector pid, port, bearer token, version | **Secret.** Readable only by the user. |
| `install.json` | Random 32-byte salt for repository and prompt hashes | **Secret.** It makes the hashes unlinkable across installs. |
| `hook-config.json` | Three booleans: store titles, store repository names, compute prompt word vectors | No |
| `spool/*.jsonl` | Minimized events waiting for the collector (max 5 MB, 2000 files) | Same as events |
| `logs/` | Daily collector and desktop logs (seven kept), and `hook.log` (rotated at 1 MB) | No prompts or code. Errors may include file paths of configuration files. |
| `backups/` | A copy of each agent configuration file before runmarq changed it | It is the user's own configuration |
| `bin/runmarq` | Stable copy of the CLI used by hooks | No |

### Run signals

Numbers only, each behind a setting (Settings → Estimation signals):

- **From transcripts** (`read_transcripts`): model responses, subagent responses, output and thinking tokens, context size and window, compactions, time of the last response. The collector reads the agent's own session file incrementally (Claude Code and Codex, `.jsonl` only) and parses each line for these numbers.
- **From git** (`sample_git_diff`): files and lines changed in the working tree since the run started, from `git diff --numstat HEAD`, and the number of tracked files (`git ls-files`) once a day per repository.
- **From hook input**: lines added and removed by edit tools, counted from the tool input in the hook process.

### Derived identifiers

- **Command keys:** `cmd_` plus 20 hex characters of SHA-256 over the installation salt and the command's program and subcommand (for example `npm test`). Arguments, paths and output never enter the hash. Stored with per-repository duration statistics in `command_stats`.
- **Prompt word vectors** (`similar_prompts`): 256 signed 8-bit values from feature-hashing the prompt's words and word pairs with a salt-derived seed. They support similarity search only; the text cannot be recovered from them. Turning the setting off deletes them.

## Never stored

- Prompt text. It is read only inside the hook process to derive `PromptFeatures`, the optional word vector and an optional title, then dropped.
- Transcript text, the transcript's path, and the run's working directory. Paths are held in the collector's memory while a run is open.
- Names of changed files from `git diff`. They are compared in memory and discarded.
- Tool input and output, file contents, command lines, environment variables, tokens.

## Agent configuration files changed (only with approval)

- **Claude Code:** `~/.claude/settings.json`, or `<project>/.claude/settings.local.json`.
- **Codex:** `~/.codex/hooks.json`, or `<project>/.codex/hooks.json`.
- **Cursor:** `~/.cursor/hooks.json`, or `<project>/.cursor/hooks.json`.

## Network

The collector listens only on 127.0.0.1. Estimation, including model training, runs locally and calls no model or API. No component makes outbound requests except:

- the website's early-access route, which forwards to a configured backend and only when one is configured;
- release tooling run by developers.
