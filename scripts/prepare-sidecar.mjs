// Builds the `runmarq` CLI in release mode and copies it to where Tauri
// expects sidecars: apps/desktop/src-tauri/binaries/runmarq-<target-triple>[.exe].
// Run before `tauri build --config src-tauri/tauri.release.conf.json`.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const target = process.env.RUNMARQ_TARGET || execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/host: (\S+)/)[1];
const ext = target.includes("windows") ? ".exe" : "";

const args = ["build", "--release", "-p", "runmarq-cli"];
if (process.env.RUNMARQ_TARGET) args.push("--target", target);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const built = process.env.RUNMARQ_TARGET ? join(root, "target", target, "release", `runmarq${ext}`) : join(root, "target", "release", `runmarq${ext}`);
const dest = join(root, "apps", "desktop", "src-tauri", "binaries", `runmarq-${target}${ext}`);
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(built, dest);
console.log(`sidecar: ${dest}`);
