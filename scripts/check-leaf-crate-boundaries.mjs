#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "..");

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

function manifestHasDependency(manifest, dependency) {
  return new RegExp(`^${dependency}\\s*=`, "mu").test(manifest);
}

export function findLeafCrateBoundaryViolations(root) {
  const violations = [];
  const workspace = read(root, "Cargo.toml") ?? "";
  const storeManifest = read(root, "crates/koushi-store/Cargo.toml");
  const coreManifest = read(root, "crates/koushi-core/Cargo.toml") ?? "";
  const qaManifest = read(root, "crates/koushi-qa/Cargo.toml") ?? "";
  const storeSources = rustSources(root, "crates/koushi-store/src");
  const coreSources = rustSources(root, "crates/koushi-core/src");
  const qaSources = rustSources(root, "crates/koushi-qa/src");

  if (!workspace.includes('"crates/koushi-store"')) {
    violations.push("workspace member missing: crates/koushi-store");
  }
  const defaultMembers = workspace.match(/default-members\s*=\s*\[([\s\S]*?)\]/mu)?.[1] ?? "";
  if (!defaultMembers.includes('"crates/koushi-store"')) {
    violations.push("default workspace member missing: crates/koushi-store");
  }

  if (storeManifest === null) {
    violations.push("missing crates/koushi-store/Cargo.toml");
  } else {
    for (const dependency of [
      "koushi-core",
      "koushi-qa",
      "koushi-sdk",
      "matrix-sdk",
      "matrix-sdk-base",
      "matrix-sdk-ui",
      "tauri",
      "tokio",
      "keyring"
    ]) {
      if (manifestHasDependency(storeManifest, dependency)) {
        violations.push(`koushi-store has forbidden dependency: ${dependency}`);
      }
    }
  }

  for (const required of [
    "pub enum CredentialStoreBackend",
    "pub struct CredentialVaultFile",
    "pub fn encrypt_envelope",
    "pub fn decrypt_envelope",
    "pub fn atomic_replace_file"
  ]) {
    if (!storeSources.some(([, source]) => source.includes(required))) {
      violations.push(`koushi-store definition missing: ${required}`);
    }
  }

  if (!manifestHasDependency(coreManifest, "koushi-store")) {
    violations.push("koushi-core must depend on koushi-store");
  }
  if (manifestHasDependency(coreManifest, "chacha20poly1305")) {
    violations.push("koushi-core still depends on chacha20poly1305");
  }
  if (!coreManifest.includes('"koushi-store/test-hooks"')) {
    violations.push("koushi-core test-hooks must enable koushi-store/test-hooks");
  }

  const joinedCore = coreSources.map(([, source]) => source).join("\n");
  if (!joinedCore.includes("pub struct StoreActor")) {
    violations.push("StoreActor must remain in koushi-core");
  }
  for (const [relativePath, source] of coreSources) {
    for (const token of [
      "pub enum CredentialStoreBackend",
      "pub struct CredentialVaultFile",
      "use chacha20poly1305",
      "fn encrypt_read_state_outbox_payload",
      "fn decrypt_read_state_payload",
      "fn atomic_replace_file"
    ]) {
      if (source.includes(token)) {
        violations.push(`moved persistence implementation remains in Core (${token}): ${relativePath}`);
      }
    }
  }

  if (!manifestHasDependency(qaManifest, "koushi-store")) {
    violations.push("koushi-qa must depend directly on koushi-store");
  }
  if (!qaManifest.includes('"koushi-store/test-hooks"')) {
    violations.push("koushi-qa qa-bin must enable koushi-store/test-hooks");
  }
  for (const [relativePath, source] of qaSources) {
    if (source.includes("koushi_core::store::resolved_credential_backend_is_file_dir")) {
      violations.push(`koushi-qa uses removed Core credential probe: ${relativePath}`);
    }
  }

  return violations;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const violations = findLeafCrateBoundaryViolations(repositoryRoot);
  if (violations.length === 0) {
    console.log("leaf crate boundaries ok");
  } else {
    console.error("leaf crate boundary violations:");
    for (const violation of violations) console.error(`- ${violation}`);
    process.exitCode = 1;
  }
}
