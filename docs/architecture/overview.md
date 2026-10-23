# Architecture overview

```text
Claude Code      Codex          Cursor
   │ hooks         │ hooks/exec    │ hooks
   ▼               ▼               ▼
        runmarq emit  (hook process, ~1 ms of work)
        · parse vendor payload (bounded to 1 MB)
        · classify prompt in memory, then drop it
        · normalize to EventEnvelope (versioned, minimized)
        · print vendor acknowledgement, exit 0
        │ HTTP POST, loopback, bearer token
        │ (on failure: bounded spool on disk)
        ▼
   Collector (one per user, file-locked)
        · validation, dedup (event id, LRU + DB)
        · session / run routing
        · run state machine, active vs waiting time
        · estimator: forecast on events + every 15 s
        · probes (off the engine lock): transcript token counts, git diff size
        · trainer (own DB connection): refits after 3 finished runs or 10 min
        · SQLite (WAL): runs, events, forecasts, signals, command timings
        · notifications (deduplicated in notification_log)
        · SSE stream to subscribers
        │ authenticated loopback API
        ▼
   Desktop app (Tauri)          runmarq CLI
   · native side holds token    · status, doctor, history
   · tray, notifications        · integrations, collector
   · webview via commands only
```

## Processes

- **Hook process**: `runmarq emit --source <agent>`. A short-lived process per hook invocation. It never writes to stderr and writes to stdout only what the vendor protocol requires. It always exits 0.
- **Collector**: `runmarq collector start --foreground`, normally started detached by the desktop app or `runmarq collector start`. It holds `collector.lock` for its lifetime, binds `127.0.0.1:0`, and writes `collector.json` (pid, port, token).
- **Desktop app**: Tauri. Its Rust side starts or reconnects to the collector, relays the SSE stream to the webview, and owns the tray and notifications. Quitting the app stops the collector; hooks then spool, and the spool is replayed with the original timestamps on the next start.

## Data model

SQLite tables (`crates/storage/src/migrations.rs`): `agents`, `repositories`, `sessions`, `runs`, `events`, `forecasts`, `run_signals`, `command_stats`, `integration_installations`, `user_preferences`, `notification_log`. All timestamps are UTC milliseconds. Repositories are identified by a salted hash of the repository root; the folder name is stored only if the user allows it.

## Event model

`EventEnvelope` (`crates/core/src/event.rs`) carries the schema version, a deterministic event id, the source agent, the vendor session id, an optional vendor turn id, a timestamp and its origin, the event type, and a typed `EventPayload`. The envelope rejects unknown fields; the payload drops them, so a newer hook can talk to an older collector, and `validate()` bounds every string. Event ids hash the source, a discriminator, the raw payload and a 2-second time bucket. That way a duplicated delivery collapses into one event, while genuinely repeated actions stay distinct.

## Run routing

- Sessions are keyed by (agent, vendor session id).
- With a vendor turn id (Codex, Cursor), events join the run with that turn id. An event with an unseen turn id starts an implicit run.
- Without one (Claude Code), events join the session's open run, or the historical run whose time window contains them when they arrive late.
- A prompt that arrives while the agent is still busy continues the open run: one flagged as coming from the agent itself (a background task reporting back), one within 10 seconds of the agent's last event, or one while a tool call is in flight. Claude Code delivers messages typed during a turn at a tool boundary, so they land here.
- Any other new prompt while a run is open ends that run as `cancelled/superseded`, at its last event.
- Compaction or a subagent finishing outside a run does not start one.
- `session_ended`, an idle notification without a prior stop, a tool failure flagged as an interrupt, and wrapper process exit all end open runs as cancelled.
- At each prompt the hook records the agent process it runs under: the nearest ancestor that is not a shell, kept only when it is a known agent executable (`process::agent_ancestor`). Once a run has been quiet for 5 seconds, the collector checks that process on every tick and ends the run as `cancelled/process_exited` when it has gone, so closing a terminal mid-run does not leave the run open.
- Claude Code's task tools change one task at a time (`TaskCreate`, `TaskUpdate`). The collector keeps each run's task list and stores every change as a whole plan snapshot, so storage, training and forecasts see plans the same way for every agent.
- Twenty minutes of silence while working makes a run `unknown`. A restart makes open runs `unknown` until their next event.

## States

`created → running ⇄ waiting_for_user / waiting_for_external_resource / blocked → completed | failed | cancelled`, with `unknown` reachable from any open state and recoverable. Terminal states are final. The transition table and its tests are in `crates/core/src/run.rs`.

## Estimator

See [the public explanation](../../apps/web/app/docs/eta-explained/page.tsx) and `crates/estimator/src/lib.rs`. In brief:

- A run's events become a `Timeline` (`timeline.rs`). `Timeline::replay` turns it into segments (one tool call and the model time after it, tagged with a phase) and a `RunSnapshot` of the run at any moment. Training replays stored events at chosen moments; live forecasting replays the open run's events. Both build the same `Query` (`features.rs`), so features mean the same thing in both.
- **History baseline** (`history.rs`, `evidence.rs`): total active time is log-normal, with priors shrunk toward local history through disjoint cohort sets, effort and model as shared offsets, quantiles conditional on survival to the elapsed time, and evidence factors with reason codes. Once a run is under way, remaining time is also read from the earlier runs that outlasted it, as a smoothed mixture of their values (`kernel_quantiles`), so a history of quick answers and long tasks keeps both instead of predicting the gap between them.
- **Phase simulation** (`phase.rs`): a semi-Markov model of phase transitions, conditioned on the last tool outcome and shrunk from agent and category to global, with log-normal tool and gap durations. In-flight commands with learned durations (`command.rs`) use those instead. Remaining time is the distribution of simulated continuations.
- **Similar prompts** (`similar.rs`): nearest finished runs by salted term-vector sketch, restricted to runs that outlasted the elapsed time.
- **Learned model** (`gbm.rs`): quantile gradient-boosted trees over 60 features, trained after 100 finished runs.
- **Combination and calibration** (`training.rs`, `conformal.rs`): components are combined as a linear pool: each P10/P50/P90 is read as a two-piece normal and the combined distribution is their weighted average, so disagreeing components widen the range rather than meeting in the middle. Rolling-origin validation (fit on runs before 40/60/80% of history, forecast the next block at up to six moments per run) chooses the weights by mean P10/P50/P90 pinball loss, with every run counting once, then pulls them toward `DEFAULT_WEIGHTS` with 50 runs' worth of weight. The same forecasts supply split-conformal scores, kept per forecast-age bucket, which shift the pooled interval.
- After calibration, plan progress pulls the interval toward the time per completed step (`evidence::plan_target`). When `evidence::finishing` holds (plan complete, or tests passing with no edits since, and no tool started for 20 active seconds) and the user's own history confirms that runs showing those signs typically ended within two minutes (`Models::finishing_confirmed`, at least four such runs), the interval is replaced by what followed the same signs in training runs, and the forecast carries the `finishing_up` reason. The phase simulation scales future steps by the run's own pace (`PhaseModel::pace`).
- `EtaForecast::outlook` and `words` (`crates/core/src/forecast.rs`, mirrored by `etaText` in `packages/contracts/src/format.ts`) turn the quantiles into the headline: an outlook band at low confidence, the likely P25–P75 range at medium or high.
- `backtest.rs` replays finished runs in end order, each forecast by an estimator fitted only on earlier runs, and scores the likely range, the full range, the outlook and the pinball loss. `runmarq eval` prints it. Forecasts only see what was known at the time: a run's model is the session's until its own transcript has been read (`TrainingRun::model_at`), because runs too short to be read never get one and would otherwise leak their outcome. Live, the collector carries a session's model into its next run for the same reason.
- Successive forecasts are smoothed, unless the change is large or driven by new evidence.
- Fitting runs on a background thread with its own SQLite connection; 3,000 runs take about five seconds in a release build (`cargo run --release -p runmarq-estimator --example fit_timing`). Weight search uses at most 1,200 validation forecasts and a coarse-to-fine grid.

## Local API

All routes require the bearer token. The collector also rejects any request that has an `Origin` header, a non-loopback `Host`, or a non-JSON body on POST/PUT. Bodies are limited to 512 KB and requests to 500 events. Routes:

- Events and lifecycle:
  - `/v1/health`
  - `/v1/events`
  - `/v1/launcher/exit`
  - `/v1/stream` (SSE)
  - `/v1/shutdown`
- Runs and history:
  - `/v1/runs/active`
  - `/v1/runs`
  - `/v1/runs/{id}`
  - `/v1/history` (DELETE)
  - `/v1/export.csv`
- Data and status:
  - `/v1/repositories`
  - `/v1/insights`
  - `/v1/agents/activity`
  - `/v1/diagnostics`
  - `/v1/preferences`
