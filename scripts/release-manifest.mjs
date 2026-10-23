// Adds a release to packages/contracts/releases.json from built artifacts.
// Run by the release workflow after artifacts are uploaded:
//
//   node scripts/release-manifest.mjs --version 0.2.0 --base-url https://example.com/releases/0.2.0 \
//     --artifact windows:x86_64:msi:path/to/runmarq_0.2.0_x64.msi[:signed] ...
//
// Each artifact is checksummed here; nothing is listed without a real file.
import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const get = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const version = get("--version");
const baseUrl = get("--base-url");
if (!version || !baseUrl?.startsWith("https://")) {
  console.error("usage: --version X.Y.Z --base-url https://... --artifact platform:arch:kind:path[:signed] ...");
  process.exit(2);
}

const artifacts = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] !== "--artifact") continue;
  const [platform, architecture, kind, path, signed] = args[i + 1].split(":");
  const file = path.length === 1 ? `${path}:${signed}` : path; // tolerate Windows drive letters
  const bytes = readFileSync(file);
  artifacts.push({
    platform,
    architecture,
    kind,
    artifactUrl: `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(basename(file))}`,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    sizeBytes: statSync(file).size,
    signed: signed === "signed",
  });
}
if (!artifacts.length) {
  console.error("no --artifact given");
  process.exit(2);
}

const manifestPath = join(dirname(fileURLToPath(import.meta.url)), "..", "packages", "contracts", "releases.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
manifest.releases = [
  { version, releaseDate: new Date().toISOString().slice(0, 10), artifacts },
  ...manifest.releases.filter((r) => r.version !== version),
];
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
console.log(`releases.json: ${version} with ${artifacts.length} artifact(s)`);
