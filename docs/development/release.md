# Releasing

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json` and the package.json files, and add a `CHANGELOG` entry in `apps/web/content/changelog.ts`.
2. Tag `vX.Y.Z` and push. `.github/workflows/release.yml` builds macOS (arm64, x64), Windows x64 and Linux x64 bundles into a **draft** GitHub release. Each job first builds the `runmarq` sidecar for its target (`scripts/prepare-sidecar.mjs`).
3. After reviewing the draft, run `scripts/release-manifest.mjs` with every artifact and the release's download address as `--base-url` (the full command is in [deploy.md](deploy.md)). It computes SHA-256 checksums and sizes and adds the release to `packages/contracts/releases.json`. Mark an artifact `:signed` only if it really is.
4. Deploy the website. The download page lists only manifest entries with an https URL, a valid checksum and a size.

## Signing

Nothing in the pipeline claims a signature it does not have.

- **macOS:** a Developer ID Application certificate and notarization are required to avoid Gatekeeper warnings. Set `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific) and `APPLE_TEAM_ID`; tauri-action signs and notarizes when they are present.
- **Windows:** without an Authenticode certificate (OV or EV), SmartScreen warns on first run. Configure `bundle.windows.certificateThumbprint`, or a signing command in `tauri.conf.json`, for the signing machine.
- **Linux:** AppImage and .deb artifacts are unsigned. Publish the checksums.

## Updates

There is no automatic updater yet. If one is added, use Tauri's updater plugin with its signature verification and a pinned public key. Never ship an unsigned update channel.
