// Builds the agentscope-hook shim and copies it to where Tauri expects the
// sidecar: src-tauri/binaries/agentscope-hook-<target-triple>[.exe].
//
//   pnpm sidecar            debug build (used by `tauri dev`)
//   pnpm sidecar --release  release build (used by `tauri build`)

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const release = process.argv.includes("--release");
const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");

// The Tauri CLI sets this for cross builds; otherwise use the host triple.
const hostTriple = () =>
  execFileSync("rustc", ["-vV"], { encoding: "utf8" })
    .split("\n")
    .find((line) => line.startsWith("host:"))
    .slice("host:".length)
    .trim();
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
const crossCompiling = triple !== hostTriple();

const args = ["build", "-p", "agentscope-hook"];
if (release) args.push("--release");
if (crossCompiling) args.push("--target", triple);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const exe = triple.includes("windows") ? ".exe" : "";
const profileDir = join(root, "target", crossCompiling ? triple : "", release ? "release" : "debug");
const outDir = join(root, "apps/desktop/src-tauri/binaries");
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `agentscope-hook-${triple}${exe}`);
// Copy then rename so a running app never sees a half-written shim.
copyFileSync(join(profileDir, `agentscope-hook${exe}`), `${out}.tmp`);
renameSync(`${out}.tmp`, out);
console.log(`sidecar ready: ${out}`);
