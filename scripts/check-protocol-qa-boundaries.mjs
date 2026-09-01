#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "..");

const protocolRequiredDefinitions = [
  "pub enum CoreCommand",
  "pub enum CoreEvent",
  "pub struct RequestId",
  "pub enum CoreFailure",
  "pub struct StateDelta"
];

const protocolForbiddenTokens = [
  "matrix_sdk",
  "tauri::",
  "tokio::",
  "PathBuf",
  "koushi-thumbnail://",
  "QaSetLocalDeviceBlacklisted",
  "QaRefreshDeviceKeysAndAssertKnown",
  "SyncOnce"
];

function read(root, relativePath) {
  const absolutePath = path.join(root, relativePath);
  return fs.existsSync(absolutePath) ? fs.readFileSync(absolutePath, "utf8") : null;
}

function rustSources(root, relativeDirectory) {
  const absoluteDirectory = path.join(root, relativeDirectory);
  if (!fs.existsSync(absoluteDirectory)) return [];
  return fs.readdirSync(absoluteDirectory, { withFileTypes: true }).flatMap((entry) => {
    const relativePath = path.join(relativeDirectory, entry.name);
    if (entry.isDirectory()) return rustSources(root, relativePath);
    return entry.isFile() && entry.name.endsWith(".rs")
      ? [[relativePath, fs.readFileSync(path.join(root, relativePath), "utf8")]]
      : [];
  });
}

export function findProtocolQaBoundaryViolations(root) {
  const violations = [];
  const workspace = read(root, "Cargo.toml") ?? "";
  const protocolManifest = read(root, "crates/koushi-protocol/Cargo.toml");
  const qaManifest = read(root, "crates/koushi-qa/Cargo.toml");
  const coreManifest = read(root, "crates/koushi-core/Cargo.toml") ?? "";
  const protocolSources = rustSources(root, "crates/koushi-protocol/src");
  const coreSources = rustSources(root, "crates/koushi-core/src");

  for (const member of ["crates/koushi-protocol", "crates/koushi-qa"]) {
    if (!workspace.includes(`\"${member}\"`)) {
      violations.push(`workspace member missing: ${member}`);
    }
  }

  if (protocolManifest === null) {
    violations.push("missing crates/koushi-protocol/Cargo.toml");
  } else {
    for (const dependency of [
      "koushi-key",
      "matrix-sdk",
      "matrix-sdk-base",
      "matrix-sdk-ui",
      "tauri",
      "tokio",
      "keyring",
      "libc",
      "windows-sys",
      "nix"
    ]) {
      if (new RegExp(`^${dependency}\\s*=`, "mu").test(protocolManifest)) {
        violations.push(`protocol manifest has forbidden dependency: ${dependency}`);
      }
    }
  }

  const protocolSource = protocolSources.map(([, source]) => source).join("\n");
  for (const definition of protocolRequiredDefinitions) {
    if (!protocolSource.includes(definition)) {
      violations.push(`protocol definition missing: ${definition}`);
    }
  }
  for (const [relativePath, source] of protocolSources) {
    for (const token of protocolForbiddenTokens) {
      if (source.includes(token)) {
        violations.push(`protocol source contains forbidden token ${token}: ${relativePath}`);
      }
    }
  }

  if (qaManifest === null) {
    violations.push("missing crates/koushi-qa/Cargo.toml");
  } else {
    for (const binary of ["headless-core-qa", "real-homeserver-qa"]) {
      if (!qaManifest.includes(`name = \"${binary}\"`)) {
        violations.push(`koushi-qa binary missing: ${binary}`);
      }
    }
    if (!qaManifest.includes('required-features = ["qa-bin"]')) {
      violations.push("koushi-qa binaries must require qa-bin");
    }
  }

  for (const legacyPath of [
    "crates/koushi-core/src/bin/headless-core-qa.rs",
    "crates/koushi-core/src/bin/headless_core_qa",
    "crates/koushi-core/src/bin/real-homeserver-qa.rs",
    "crates/koushi-core/src/bin/real_homeserver_qa"
  ]) {
    if (fs.existsSync(path.join(root, legacyPath))) {
      violations.push(`QA source remains in koushi-core: ${legacyPath}`);
    }
  }

  for (const binary of ["headless-core-qa", "real-homeserver-qa"]) {
    if (coreManifest.includes(`name = \"${binary}\"`)) {
      violations.push(`koushi-core still declares QA binary: ${binary}`);
    }
  }
  if (/^qa-bin\s*=/mu.test(coreManifest)) {
    violations.push("koushi-core still declares qa-bin feature");
  }

  for (const [relativePath, source] of coreSources) {
    if (source.includes("koushi-thumbnail://")) {
      violations.push(`Core mints Tauri thumbnail URI: ${relativePath}`);
    }
    for (const definition of protocolRequiredDefinitions) {
      if (source.includes(definition)) {
        violations.push(`protocol definition remains in Core (${definition}): ${relativePath}`);
      }
    }
  }

  return violations;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const violations = findProtocolQaBoundaryViolations(repositoryRoot);
  if (violations.length === 0) {
    console.log("protocol/QA boundaries ok");
  } else {
    console.error("protocol/QA boundary violations:");
    for (const violation of violations) console.error(`- ${violation}`);
    process.exitCode = 1;
  }
}
