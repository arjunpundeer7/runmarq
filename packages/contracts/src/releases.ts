// Release manifest. Written by the release workflow after artifacts are
// uploaded and checksummed; read by the website's download page.
import data from "../releases.json";

export type Platform = "macos" | "windows" | "linux";
export type Architecture = "aarch64" | "x86_64";

export interface ReleaseArtifact {
  platform: Platform;
  architecture: Architecture;
  /** Installer kind, for example "dmg", "msi", "nsis", "appimage", "deb". */
  kind: string;
  artifactUrl: string;
  sha256: string;
  sizeBytes: number;
  /** True only when the artifact is code-signed (and notarized on macOS). */
  signed: boolean;
}

export interface Release {
  version: string;
  releaseDate: string;
  notesUrl?: string;
  artifacts: ReleaseArtifact[];
}

export interface ReleaseManifest {
  releases: Release[];
}

const SHA256 = /^[0-9a-f]{64}$/;

/** Keeps only entries that look like real, verifiable artifacts. */
export function validReleases(m: ReleaseManifest): Release[] {
  return m.releases
    .map((r) => ({
      ...r,
      artifacts: r.artifacts.filter((a) => a.artifactUrl.startsWith("https://") && SHA256.test(a.sha256) && a.sizeBytes > 0),
    }))
    .filter((r) => r.artifacts.length > 0);
}

export const RELEASES: Release[] = validReleases(data as ReleaseManifest);
export const LATEST_RELEASE: Release | null = RELEASES[0] ?? null;
