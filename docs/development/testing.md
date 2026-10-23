# Testing

| Suite | Command | Covers |
|---|---|---|
| Core | `cargo test -p runmarq-core` | Envelope validation, unknown envelope fields rejected and unknown payload fields dropped, event-id stability, the state transition table, forecast serialization and humanized ranges |
| Storage | `cargo test -p runmarq-storage` | Migrations and WAL, in-place upgrade of version 1 databases, refusing newer schemas, round-trips, bulk event and signal loading, command timings, sketch erasure, duplicate events, parameterized search including injection strings, cascading deletes, notification dedup |
| Estimator | `cargo test -p runmarq-estimator` | Ordered and non-negative quantiles everywhere, survival conditioning, cold start, insufficient data, waiting, repeated failures, smoothing, outliers, per-agent learning, effort offsets, timeline replay and active time, phase transitions reacting to test outcomes, in-flight command durations, similar-prompt separation, the learned model activating only after 100 runs, quantile boosting coverage, conformal widening and narrowing, a **calibration simulation** (start and conditional coverage), temporal holdout, withheld metrics |
| Adapters | `cargo test -p runmarq-adapters` | Fixture replay for all three agents, transcript parsing for Claude Code and Codex, command signatures and salted keys, edit sizes, effort and session context, no leakage of sample secrets, hostile payloads, install/verify/uninstall round-trips that restore the original bytes, refusing corrupt config, project scope, manifest freshness |
| Collector | `cargo test -p runmarq-collector` | An emitted run with its persisted duration, the full fixture lifecycles, interrupts, supersession, idle inference, concurrency, duplicates, out-of-order and late events, restart recovery, staleness, waiting, launcher exit, forecast reaction, history learning, transcript signals without stored paths, JSON Lines-only transcript reads, command timing, training data from timelines, sketch erasure, incremental and truncated transcript reads, diff growth against a real git repository, preferences, quiet mode, and HTTP security (token, Origin, Host, content type, size, single instance, spool replay, SSE) |
| Desktop native | `cargo test -p runmarq-desktop` | Query encoding, tray summaries |
| Shared TS | `pnpm --filter @runmarq/contracts test`, `pnpm --filter @runmarq/ui test` | Formatter parity with Rust, manifest, release validation, run card states |
| Desktop UI | `pnpm --filter @runmarq/desktop test` | Overview, empty and offline states, live stream updates, navigation, run detail with run activity and models used, estimation model status, integrations honesty, setup preview, settings persistence including estimation signals, onboarding |
| Website unit | `pnpm --filter @runmarq/web test` | Sign-up validation, rate limiting |
| Website e2e | `pnpm --filter @runmarq/web build && pnpm test:e2e` | Every route at desktop and phone sizes (one h1, no horizontal overflow), 404, broken internal links, pre-release download state, early-access refusal, demo interaction, integration tabs, docs search, keyboard path, mobile nav |

## Fixtures

The fixtures in `fixtures/` are contract-derived; see `fixtures/README.md`. To capture real payloads, set `RUNMARQ_CAPTURE_DIR`.

## Measured performance (development machine, Windows 11, release build)

- `runmarq emit`, end to end from Git Bash including process start-up: 71–91 ms per hook.
- Collector-side processing (diagnostics, p50/p95): hook 0.3/0.5 ms, ingest 0.4/0.5 ms.
- Forecast with all four models active: about 0.9 ms. Fitting on 3,000 synthetic runs: about 2 s on a background thread (`cargo run --release -p runmarq-estimator --example fit_timing`).

`runmarq doctor` and Settings → Diagnostics report these figures for any machine.
