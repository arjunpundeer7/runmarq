## What and why

<!-- A short description of the change and the motivation. -->

## Checklist

- [ ] `pnpm check` passes
- [ ] Rust types exported to TypeScript changed → ran `pnpm contracts` and committed the output
- [ ] Adapter capabilities changed → regenerated `packages/contracts/integrations.json`
- [ ] Engine or estimator changed → ran `pnpm demo`
- [ ] UI changed → checked light/dark, keyboard focus, reduced motion and 390px width
- [ ] No prompt text, code, commands or output can reach an event, log or the database
