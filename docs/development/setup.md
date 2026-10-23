# Development setup

## Toolchain

- Rust stable (`rustup`, with the `clippy` and `rustfmt` components)
- Node.js 20.9 or later and pnpm 9 (`npm install -g pnpm@9` or `corepack enable`)
- Tauri prerequisites for your platform:
  - **Windows:** Visual Studio Build Tools with the C++ workload, and WebView2. WebView2 is preinstalled on Windows 11.
  - **macOS:** Xcode Command Line Tools.
  - **Linux:** `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf build-essential`.

## First run

```bash
pnpm install
cargo build -p runmarq-cli
cargo test --workspace
```

## Working on each part

| Part | Command | Notes |
|---|---|---|
| Collector and CLI | `cargo run -p runmarq-cli -- collector start --foreground` | Use `RUNMARQ_HOME=$PWD/.dev-data` to sandbox data |
| Hooks by hand | `cat fixtures/claude/basic-run.jsonl \| while read l; do echo "$l" \| target/debug/runmarq emit --source claude_code; done` | Feeds fixture payloads through the real hook path |
| Desktop app | `pnpm dev:desktop` | Vite on 127.0.0.1:1420 plus the Tauri shell. The app finds the newest `target/*/runmarq` and copies it into `<data>/bin` |
| Desktop UI only | `pnpm --filter @runmarq/desktop dev`, then open `http://127.0.0.1:1420/?mock` | Sample data, no Tauri. Add `&empty`, `&offline` or `&onboarding` for those states |
| Website | `pnpm dev:web` | Next.js on port 3000 |

## Sandboxing

`RUNMARQ_HOME` redirects every file runmarq writes. Agent configuration files are not redirected: `integrations setup` edits the real `~/.claude`, `~/.codex` or `~/.cursor` unless you pass `--project <dir>`. For experiments, use `--project` with a scratch folder, or `--dry-run`.

## Generated files

- `packages/contracts/src/generated/`: run `pnpm contracts` after changing exported Rust types.
- `packages/contracts/integrations.json`: run `cargo run -p runmarq-cli -- integrations manifest > packages/contracts/integrations.json`.
- `apps/web/content/demo-run.json`: run `pnpm demo`.
