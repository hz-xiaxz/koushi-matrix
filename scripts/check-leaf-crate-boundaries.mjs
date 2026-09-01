#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "..");

export const testkitTargets = [
  "command_redaction.rs",
  "composer_draft_lifecycle.rs",
  "composer_draft_wire.rs",
  "dm_space_ids.rs",
  "event_redaction.rs",
  "local_store_migration.rs",
  "login_store_lifecycle.rs",
  "media_staging.rs",
  "media_staging_b2.rs",
  "pending_login_journal.rs",
  "request_outcome.rs",
  "request_outcome_a2a.rs",
  "request_outcome_a2b.rs",
  "request_outcome_a2c.rs",
  "room_subscription_residency.rs",
  "runtime_account_management.rs",
  "runtime_activity.rs",
  "runtime_command_admission.rs",
  "runtime_core.rs",
  "runtime_e2ee.rs",
  "runtime_intent_lifecycle.rs",
  "runtime_navigation.rs",
  "runtime_notification_settings.rs",
  "runtime_room_list_sync.rs",
  "runtime_room_preferences.rs",
  "runtime_room_selection_scale.rs",
  "runtime_scheduled_send.rs",
  "runtime_search.rs",
  "runtime_session.rs",
  "runtime_settings.rs",
  "runtime_timeline.rs",
  "send_queue_fast.rs"
];

export const coreLocalIntegrationTargets = [
  "link_preview.rs",
  "media_save.rs",
  "native_artifact_boundary.rs",
  "sliding_sync_diagnostics.rs"
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

function manifestHasDependency(manifest, dependency) {
  return new RegExp(`^${dependency}\\s*=`, "mu").test(manifest);
}

function directRustFiles(root, relativeDirectory) {
  const directory = path.join(root, relativeDirectory);
  if (!fs.existsSync(directory)) return [];
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
    .map((entry) => entry.name)
    .sort();
}

export function findLeafCrateBoundaryViolations(root) {
  const violations = [];
  const workspace = read(root, "Cargo.toml") ?? "";
  const storeManifest = read(root, "crates/koushi-store/Cargo.toml");
  const coreManifest = read(root, "crates/koushi-core/Cargo.toml") ?? "";
  const qaManifest = read(root, "crates/koushi-qa/Cargo.toml") ?? "";
  const testkitManifest = read(root, "crates/koushi-core-testkit/Cargo.toml");
  const ci = read(root, ".github/workflows/ci.yml") ?? "";
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
  if (!workspace.includes('"crates/koushi-core-testkit"')) {
    violations.push("workspace member missing: crates/koushi-core-testkit");
  }
  if (defaultMembers.includes('"crates/koushi-core-testkit"')) {
    violations.push("koushi-core-testkit must not be a default workspace member");
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

  if (testkitManifest === null) {
    violations.push("missing crates/koushi-core-testkit/Cargo.toml");
  } else {
    if (!/^publish\s*=\s*false$/mu.test(testkitManifest)) {
      violations.push("koushi-core-testkit must be publish-disabled");
    }
    if (
      !/^koushi-core\s*=\s*\{[^}]*features\s*=\s*\[\s*"test-hooks"\s*\][^}]*\}/mu.test(
        testkitManifest
      )
    ) {
      violations.push("koushi-core-testkit must enable koushi-core/test-hooks");
    }
  }
  if (/^koushi-core\s*=\s*\{[^}]*path\s*=\s*"\."[^}]*\}/mu.test(coreManifest)) {
    violations.push("koushi-core must not self-depend for test hooks");
  }
  for (const [owner, manifest] of [
    ["koushi-core", coreManifest],
    ["koushi-qa", qaManifest],
    ["koushi-store", storeManifest ?? ""]
  ]) {
    if (manifestHasDependency(manifest, "koushi-core-testkit")) {
      violations.push(`${owner} must not depend on koushi-core-testkit`);
    }
  }
  if (!fs.existsSync(path.join(root, "crates/koushi-core-testkit/tests/support/mod.rs"))) {
    violations.push("shared Core integration support missing from koushi-core-testkit");
  }
  if (fs.existsSync(path.join(root, "crates/koushi-core/tests/support"))) {
    violations.push("shared integration support remains in koushi-core");
  }
  for (const target of testkitTargets) {
    if (!fs.existsSync(path.join(root, "crates/koushi-core-testkit/tests", target))) {
      violations.push(`koushi-core-testkit target missing: ${target}`);
    }
    if (fs.existsSync(path.join(root, "crates/koushi-core/tests", target))) {
      violations.push(`moved integration target remains in koushi-core: ${target}`);
    }
  }
  for (const target of coreLocalIntegrationTargets) {
    if (!fs.existsSync(path.join(root, "crates/koushi-core/tests", target))) {
      violations.push(`Core-local integration target missing: ${target}`);
    }
    if (fs.existsSync(path.join(root, "crates/koushi-core-testkit/tests", target))) {
      violations.push(`Core-local integration target moved unnecessarily: ${target}`);
    }
  }
  const coreTargets = directRustFiles(root, "crates/koushi-core/tests");
  if (JSON.stringify(coreTargets) !== JSON.stringify([...coreLocalIntegrationTargets].sort())) {
    violations.push("unexpected Rust integration target set in koushi-core");
  }
  const movedTargets = directRustFiles(root, "crates/koushi-core-testkit/tests");
  if (JSON.stringify(movedTargets) !== JSON.stringify([...testkitTargets].sort())) {
    violations.push("unexpected Rust integration target set in koushi-core-testkit");
  }
  if (!ci.includes("cargo test -p koushi-core-testkit")) {
    violations.push("CI must run koushi-core-testkit explicitly");
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
    for (const token of [
      '#[cfg(feature = "test-hooks")]',
      '#[cfg(not(feature = "test-hooks"))]',
      '#[cfg(all(test, feature = "test-hooks"))]'
    ]) {
      if (source.includes(token)) {
        violations.push(`Core test-hook cfg excludes unit tests (${token}): ${relativePath}`);
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
