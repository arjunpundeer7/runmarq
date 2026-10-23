# Contributing

## Ground rules

- **Never fake compatibility.** Integrations use documented vendor interfaces only. If an agent cannot report something, the capability is `false` in the adapter, the UI hides or qualifies the feature, and the limitation is listed. Check the installed vendor version and its documentation before changing an adapter.
- **Never weaken privacy defaults.** Prompt text, code, commands and output must not reach an event, a log or the database. New `EventPayload` fields need a privacy review; the adapter tests check that sample secrets do not leak.
- **Never turn ambiguity into success.** A run without a reported end is `cancelled` or `unknown`, never `completed`.
- **No invented numbers.** Accuracy metrics stay hidden below the minimum sample size; the website shows only manifest-backed capabilities and real releases.

## Workflow

1. `pnpm install`, then `pnpm check` before opening a pull request.
2. Changing a Rust type exported to TypeScript: run `pnpm contracts` and commit the generated files.
3. Changing an adapter's capabilities or limitations: run `cargo run -p runmarq-cli -- integrations manifest > packages/contracts/integrations.json`; a test fails otherwise.
4. Changing the engine or estimator: run `pnpm demo` so the website demonstration reflects real behaviour.
5. Changing UI: check light and dark themes, keyboard focus, reduced motion and a 390px-wide viewport.

## Adding an agent

1. Add `crates/adapters/src/<agent>.rs` implementing `Adapter` and register it in `all()` / `for_source()`.
2. Add the source to `AgentSource` in `crates/core/src/event.rs`.
3. Add fixtures under `fixtures/<agent>/` from documented or captured payloads (see `fixtures/README.md`) and tests in `crates/adapters/tests/fixtures.rs`.
4. Regenerate the manifest and contracts. The collector, storage and estimator need no changes.

## Style

Rust: `cargo fmt`, Clippy clean with `-D warnings`. TypeScript: strict mode, ESLint clean. Comments explain why, not what.
