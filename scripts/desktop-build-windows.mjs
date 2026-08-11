#!/usr/bin/env node
// Windows x64 NSIS trial build entry point (issue #441 Phase A).
//
// Analogous to desktop-build-dmg.mjs. Produces ONE unsigned NSIS installer:
//   tauri build --bundles nsis --target x86_64-pc-windows-msvc
// then validates the output (exactly one, non-empty, freshly generated) and
// prints the absolute path + SHA-256. The artifact is an unsigned trial
// build: never implies signing, never publishes a release asset.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const desktopDir = join(repoRoot, "apps", "desktop");
const nsisDir = join(
  repoRoot,
  "target",
  "x86_64-pc-windows-msvc",
  "release",
  "bundle",
  "nsis"
);
const args = new Set(process.argv.slice(2));
// Windows has no bare npm executable: spawnSync must use the .cmd shim.
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

if (args.has("--help")) {
  printUsage();
  process.exit(0);
}

if (process.platform !== "win32" && !args.has("--print-command")) {
  console.error("desktop-build-windows: Windows NSIS bundling is only available on Windows.");
  process.exit(1);
}

printStorageNotice();

const buildCommand = [
  "run",
  "tauri",
  "--",
  "build",
  "--bundles",
  "nsis",
  "--target",
  "x86_64-pc-windows-msvc"
];
if (args.has("--print-command")) {
  console.log(`desktop-build-windows: npm ${buildCommand.join(" ")}`);
  process.exit(0);
}

if (!args.has("--skip-preflight")) {
  run("node", ["scripts/desktop-release-preflight.mjs", "--check-config"], repoRoot);
}

const buildStartMs = Date.now();
run(npmCommand, buildCommand, desktopDir);

const installers = listNsisInstallers();
if (installers.length !== 1) {
  console.error(
    `desktop-build-windows: expected exactly one NSIS installer under ${nsisDir}, found ${installers.length}`
  );
  process.exit(1);
}

const installer = installers[0];
const stat = statSync(installer);
if (stat.size === 0) {
  console.error(`desktop-build-windows: installer is empty: ${installer}`);
  process.exit(1);
}
if (stat.mtimeMs < buildStartMs) {
  console.error(`desktop-build-windows: installer is stale (not produced by this run): ${installer}`);
  process.exit(1);
}

const sha256 = sha256Of(installer);
console.log("desktop-build-windows: UNSIGNED trial installer (not code-signed; Windows SmartScreen may warn)");
console.log("  artifact: " + installer);
console.log("  sha256:   " + sha256);

function run(command, commandArgs, cwd) {
  const result = spawnSync(command, commandArgs, {
    cwd,
    stdio: "inherit",
    env: process.env
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function listNsisInstallers() {
  if (!existsSync(nsisDir)) {
    return [];
  }
  return readdirSync(nsisDir)
    .filter((file) => file.endsWith(".exe"))
    .sort()
    .map((file) => join(nsisDir, file));
}

function sha256Of(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function printStorageNotice() {
  console.log("desktop-build-windows: local installed-app storage");
  console.log("  data: %APPDATA%\\koushi-desktop (encrypted Matrix store/search/cache)");
  console.log("  credential service: Windows Credential Manager (koushi-desktop)");
}

function printUsage() {
  console.log("Usage: npm --prefix apps/desktop run build:windows [-- --print-command|--skip-preflight]");
  console.log("Builds the unsigned Windows x64 NSIS trial installer via Tauri.");
  printStorageNotice();
}
