#!/usr/bin/env node
// Linux x86_64 AppImage + deb trial build entry point.
//
// Analogous to desktop-build-windows.mjs. Produces ONE AppImage, ONE
// Debian package, and ONE RPM package:
//   tauri build --bundles appimage,deb,rpm --target x86_64-unknown-linux-gnu
// then validates the outputs (exactly one each, non-empty, freshly
// generated) and prints the absolute paths + SHA-256. The artifacts are
// unsigned trial builds: never implies signing, never publishes a release
// asset.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const desktopDir = join(repoRoot, "apps", "desktop");
const target = "x86_64-unknown-linux-gnu";
const bundleRoot = join(repoRoot, "target", target, "release", "bundle");
const appimageDir = join(bundleRoot, "appimage");
const debDir = join(bundleRoot, "deb");
const rpmDir = join(bundleRoot, "rpm");
const args = new Set(process.argv.slice(2));

if (args.has("--help")) {
  printUsage();
  process.exit(0);
}

if (process.platform !== "linux" && !args.has("--print-command")) {
  console.error("desktop-build-linux: Linux bundling is only available on Linux.");
  process.exit(1);
}

printStorageNotice();

const buildCommand = [
  "run",
  "tauri",
  "--",
  "build",
  "--bundles",
  "appimage,deb,rpm",
  "--target",
  target
];
if (args.has("--print-command")) {
  console.log(`desktop-build-linux: npm ${buildCommand.join(" ")}`);
  process.exit(0);
}

if (!args.has("--skip-preflight")) {
  run("node", ["scripts/desktop-release-preflight.mjs", "--check-config"], repoRoot);
}

const buildStartMs = Date.now();
run("npm", buildCommand, desktopDir);

const appimages = listArtifacts(appimageDir, ".AppImage");
const debs = listArtifacts(debDir, ".deb");
const rpms = listArtifacts(rpmDir, ".rpm");
validateSingleArtifact(appimages, appimageDir, "AppImage");
validateSingleArtifact(debs, debDir, "deb package");
validateSingleArtifact(rpms, rpmDir, "RPM package");

console.log("desktop-build-linux: UNSIGNED trial artifacts (not code-signed)");
for (const artifact of [appimages[0], debs[0], rpms[0]]) {
  console.log("  artifact: " + artifact);
  console.log("  sha256:   " + sha256Of(artifact));
}

function validateSingleArtifact(found, dir, label) {
  if (found.length !== 1) {
    console.error(
      `desktop-build-linux: expected exactly one ${label} under ${dir}, found ${found.length}`
    );
    process.exit(1);
  }
  const stat = statSync(found[0]);
  if (stat.size === 0) {
    console.error(`desktop-build-linux: ${label} is empty: ${found[0]}`);
    process.exit(1);
  }
  if (stat.mtimeMs < buildStartMs) {
    console.error(
      `desktop-build-linux: ${label} is stale (not produced by this run): ${found[0]}`
    );
    process.exit(1);
  }
}

function run(command, commandArgs, cwd) {
  const result = spawnSync(command, commandArgs, {
    cwd,
    stdio: "inherit",
    env: process.env
  });
  if (result.error) {
    console.error(`desktop-build-linux: failed to spawn ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function listArtifacts(dir, extension) {
  if (!existsSync(dir)) {
    return [];
  }
  return readdirSync(dir)
    .filter((file) => file.endsWith(extension))
    .sort()
    .map((file) => join(dir, file));
}

function sha256Of(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function printStorageNotice() {
  console.log("desktop-build-linux: local installed-app storage");
  console.log("  data: ~/.local/share/koushi-desktop (encrypted Matrix store/search/cache)");
  console.log("  credential service: Secret Service / keyring (koushi-desktop)");
}

function printUsage() {
  console.log("Usage: npm --prefix apps/desktop run build:linux [-- --print-command|--skip-preflight]");
  console.log("Builds the unsigned Linux x86_64 AppImage, deb, and RPM trial artifacts via Tauri.");
  printStorageNotice();
}
