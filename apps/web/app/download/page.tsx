import type { Architecture, Platform, ReleaseArtifact } from "@runmarq/contracts";
import { LATEST_RELEASE, RELEASES } from "@runmarq/contracts";
import type { Metadata } from "next";
import { CodeBlock } from "@/components/CodeBlock";
import { EarlyAccessForm } from "@/components/EarlyAccessForm";
import { ContentPage, MoreLink, Note, PageTitle } from "@/components/Prose";
import { earlyAccessConfigured, pageMetadata, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata(
  "/download",
  "Download",
  LATEST_RELEASE
    ? `Download ${site.name} ${LATEST_RELEASE.version} for Windows, macOS and Linux, with checksums for every installer.`
    : `Build ${site.name} from source for Windows, macOS and Linux. Installers come with the first release.`,
);

const PLATFORM: Record<Platform, string> = { macos: "macOS", windows: "Windows", linux: "Linux" };
const ARCH: Record<Architecture, string> = { aarch64: "Apple silicon / ARM64", x86_64: "x64" };
const KIND: Record<string, string> = {
  nsis: "Installer (.exe)",
  msi: "MSI package",
  dmg: "Disk image (.dmg)",
  appimage: "AppImage",
  deb: "Debian package (.deb)",
};

const PLATFORMS = [
  ["Windows 10 and 11, x64", "Built and tested"],
  ["macOS, Apple silicon and Intel", "Builds; release testing pending"],
  ["Linux x64 with WebKitGTK 4.1", "Builds; release testing pending"],
];

function size(bytes: number) {
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

function ArtifactRow({ a }: { a: ReleaseArtifact }) {
  return (
    <li className="grid gap-3 rounded-[18px] bg-surface p-5 shadow-[0_0_0_1px_var(--rm-border)] md:grid-cols-[1fr_auto] md:items-center">
      <div>
        <p className="text-[17px] font-semibold">
          {PLATFORM[a.platform]} · {ARCH[a.architecture]}
        </p>
        <p className="mt-0.5 text-[14px] text-muted">
          {KIND[a.kind] ?? a.kind.toUpperCase()} · {size(a.sizeBytes)} · {a.signed ? "Signed" : "Not code-signed"}
        </p>
        <p className="mt-2 break-all text-[13px] text-muted">
          SHA-256 <code className="font-mono text-fg-2">{a.sha256}</code>
        </p>
      </div>
      <a href={a.artifactUrl} className="btn btn-primary btn-sm">
        Download
      </a>
    </li>
  );
}

export default function Download() {
  return (
    <ContentPage>
      <PageTitle title={LATEST_RELEASE ? `Download ${site.name} ${LATEST_RELEASE.version}` : "Download"} />

      {LATEST_RELEASE ? (
        <>
          <ul className="max-w-3xl space-y-2">
            {LATEST_RELEASE.artifacts.map((a) => (
              <ArtifactRow key={a.artifactUrl} a={a} />
            ))}
          </ul>
          <section aria-labelledby="verify" className="mt-12 max-w-3xl">
            <h2 id="verify" className="text-[32px] leading-tight">
              Verify a download
            </h2>
            <CodeBlock label="macOS and Linux">{`shasum -a 256 <file>`}</CodeBlock>
            <CodeBlock label="Windows PowerShell">{`Get-FileHash <file> -Algorithm SHA256`}</CodeBlock>
            {LATEST_RELEASE.artifacts.some((a) => !a.signed) && (
              <Note tone="warn">
                Builds marked “Not code-signed” show an operating-system warning on first launch. Compare the checksum before running them.
              </Note>
            )}
            {RELEASES.length > 1 && <MoreLink href="/changelog">Earlier versions</MoreLink>}
          </section>
        </>
      ) : (
        <div className={`grid gap-10 ${earlyAccessConfigured ? "max-w-5xl md:grid-cols-2" : "max-w-3xl"}`}>
          <section id="source" aria-labelledby="source-title" className="min-w-0 scroll-mt-28">
            <h2 id="source-title" className="text-[32px] leading-tight">
              Build it from source
            </h2>
            <p className="mt-3 text-[17px] text-fg-2">
              {site.name} has not had a public release yet. It builds on Windows, macOS and Linux with Rust, Node.js 20.9 or later and pnpm,
              and installers built this way are not code-signed.
            </p>
            <CodeBlock>{`pnpm install
cargo build --release -p runmarq-cli
pnpm build:desktop`}</CodeBlock>
            <MoreLink href="/docs/getting-started">Getting started</MoreLink>
          </section>

          {earlyAccessConfigured && (
            <section id="early-access" aria-labelledby="ea" className="rm-panel min-w-0 scroll-mt-28 self-start p-6 sm:p-8">
              <h2 id="ea" className="mb-6 text-[32px] leading-tight">
                Get early access
              </h2>
              <EarlyAccessForm />
            </section>
          )}
        </div>
      )}

      <section aria-labelledby="platforms" className="mt-20 max-w-3xl">
        <h2 id="platforms" className="text-[32px] leading-tight">
          Platforms
        </h2>
        <ul className="mt-4 space-y-1">
          {PLATFORMS.map(([p, s], i) => (
            <li key={p} className={`flex flex-wrap justify-between gap-2 rounded-[12px] px-4 py-3.5 ${i % 2 === 0 ? "bg-surface-2" : "bg-surface-2/50"}`}>
              <span className="text-[16px] font-medium">{p}</span>
              <span className="text-[15px] text-muted">{s}</span>
            </li>
          ))}
        </ul>
      </section>
    </ContentPage>
  );
}
