# Deploying

The website runs on Vercel. The desktop app and the `runmarq` command are published as GitHub Releases, which the website's download page links to.

## Website (Vercel)

1. Push the repository to GitHub.
2. In Vercel, add a project from that repository with these settings:
   - **Root Directory:** `apps/web`. Vercel finds the pnpm workspace above it and installs from the repository root.
   - **Framework preset:** Next.js. The build command (`next build`) and output come from the preset.
   - **Node.js:** 20 or later.
3. Set the environment variables under Settings → Environment Variables. They are read at build time, so redeploy after changing one.

   | Variable | Purpose |
   | --- | --- |
   | `SITE_URL` | The public address, such as `https://runmarq.dev`. Used by the sitemap, robots.txt, llms.txt and link previews. Without it, Vercel's production domain is used. |
   | `GITHUB_URL` | The repository's address. Shows GitHub in the header, footer and contact page. |
   | `CONTACT_EMAIL` | Shown on the contact and privacy pages. |
   | `EARLY_ACCESS_WEBHOOK_URL`, `EARLY_ACCESS_WEBHOOK_SECRET` | Optional. Opens the early-access form; see `.env.example`. Without them, the main button reads "Build from source" until a release exists. |

4. Add the domain under Settings → Domains.

Each push to the default branch deploys to production; pull requests get preview deployments. Production responses carry a content security policy and HSTS (`apps/web/next.config.ts`). The site makes no third-party requests.

To check a production build locally:

```sh
pnpm --filter @runmarq/web build
pnpm --filter @runmarq/web exec next start --port 3210
pnpm --filter @runmarq/web exec playwright test
```

## Desktop app and command (GitHub Releases)

1. Bump the version and add a changelog entry as described in [release.md](release.md).
2. Tag `vX.Y.Z` and push the tag. `.github/workflows/release.yml` builds these on GitHub's machines into a **draft** release:
   - Windows x64: `setup.exe` and `.msi`
   - macOS Apple silicon and Intel: `.dmg`
   - Linux x64: `.AppImage` and `.deb`
3. Download the draft's assets, try each installer, then publish the release.
4. Record the artifacts for the download page, using the release's own download address as the base URL:

   ```sh
   node scripts/release-manifest.mjs --version X.Y.Z \
     --base-url https://github.com/OWNER/REPO/releases/download/vX.Y.Z \
     --artifact windows:x86_64:nsis:runmarq_X.Y.Z_x64-setup.exe \
     --artifact windows:x86_64:msi:runmarq_X.Y.Z_x64_en-US.msi \
     --artifact macos:aarch64:dmg:runmarq_X.Y.Z_aarch64.dmg \
     --artifact macos:x86_64:dmg:runmarq_X.Y.Z_x64.dmg \
     --artifact linux:x86_64:appimage:runmarq_X.Y.Z_amd64.AppImage \
     --artifact linux:x86_64:deb:runmarq_X.Y.Z_amd64.deb
   ```

   Add `:signed` to an artifact only if it really is signed.
5. Commit `packages/contracts/releases.json`. The next website deploy lists the downloads with their checksums and changes the main button to "Download runmarq".

Builds are unsigned until certificates are set up (see [release.md](release.md#signing)). Windows shows "Unknown publisher" on first launch, and macOS needs right-click → Open. The download page says so for every unsigned artifact.

To build the Windows installer locally:

```sh
node scripts/prepare-sidecar.mjs
pnpm --filter @runmarq/desktop tauri build --config src-tauri/tauri.release.conf.json --bundles nsis
```

The installer lands in `target/release/bundle/nsis/`. It installs for the current user only and needs no administrator rights.
