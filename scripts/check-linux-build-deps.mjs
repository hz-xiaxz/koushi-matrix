#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Package names are for Debian/Ubuntu; other distributions use the same
// pkg-config modules but may name their development packages differently.
const libraries = [
  ["gtk+-3.0", "libgtk-3-dev"],
  ["webkit2gtk-4.1", "libwebkit2gtk-4.1-dev"],
  ["ayatana-appindicator3-0.1", "libayatana-appindicator3-dev"],
  ["librsvg-2.0", "librsvg2-dev"],
  ["openssl", "libssl-dev"],
  ["dbus-1", "libdbus-1-dev"],
];

export function checkLinuxBuildDeps(probe = spawnSync) {
  const missing = [];
  const available = (command, args) =>
    probe(command, args, { stdio: "ignore", timeout: 10_000 }).status === 0;
  for (const [command, label] of [
    ["cc", "cc (build-essential)"],
    ["c++", "c++ (build-essential)"],
    ["make", "make (build-essential)"],
    ["patchelf", "patchelf"],
  ]) {
    if (!available(command, ["--version"])) missing.push(label);
  }
  if (!available("pkg-config", ["--version"])) {
    missing.push("pkg-config");
  } else {
    for (const [module, pkg] of libraries) {
      if (!available("pkg-config", ["--exists", module])) {
        missing.push(`${module} (${pkg})`);
      }
    }
  }
  return missing;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.platform !== "linux") {
    console.error("Linux build dependency checks must run inside Linux/WSL.");
    process.exitCode = 1;
  } else {
    const missing = checkLinuxBuildDeps();
    if (missing.length) {
      console.error(`Missing Linux build dependencies:\n  ${missing.join("\n  ")}`);
      console.error("See README.md: Build on Linux / WSL (Ubuntu 24.04). Install the system packages, then rerun this check.");
      process.exitCode = 1;
    } else {
      console.log("Linux compiler, packaging tool, and pkg-config dependencies are available.");
    }
  }
}
