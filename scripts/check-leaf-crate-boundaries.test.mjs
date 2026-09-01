#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";

import {
  coreLocalIntegrationTargets,
  findLeafCrateBoundaryViolations,
  testkitTargets
} from "./check-leaf-crate-boundaries.mjs";

const roots = [];
afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function write(root, relativePath, source) {
  const absolutePath = path.join(root, relativePath);
  fs.mkdirSync(path.dirname(absolutePath), { recursive: true });
  fs.writeFileSync(absolutePath, source);
}

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "koushi-leaf-boundary-"));
  roots.push(root);
  write(
    root,
    "Cargo.toml",
    '[workspace]\nmembers = ["crates/koushi-store", "crates/koushi-core", "crates/koushi-core-testkit", "crates/koushi-qa"]\ndefault-members = ["crates/koushi-store", "crates/koushi-core"]\n'
  );
  write(
    root,
    "crates/koushi-store/Cargo.toml",
    '[package]\nname = "koushi-store"\n[dependencies]\nkoushi-key = { path = "../koushi-key" }\n[features]\ntest-hooks = []\n'
  );
  write(
    root,
    "crates/koushi-store/src/lib.rs",
    "pub enum CredentialStoreBackend {}\npub struct CredentialVaultFile;\npub fn encrypt_envelope() {}\npub fn decrypt_envelope() {}\npub fn atomic_replace_file() {}\n"
  );
  write(
    root,
    "crates/koushi-core/Cargo.toml",
    '[package]\nname = "koushi-core"\n[dependencies]\nkoushi-store = { path = "../koushi-store" }\n[features]\ntest-hooks = ["koushi-store/test-hooks"]\n'
  );
  write(root, "crates/koushi-core/src/lib.rs", "pub struct StoreActor;\n");
  write(
    root,
    "crates/koushi-core-testkit/Cargo.toml",
    '[package]\nname = "koushi-core-testkit"\npublish = false\n[dev-dependencies]\nkoushi-core = { path = "../koushi-core", features = ["test-hooks"] }\n'
  );
  write(root, "crates/koushi-core-testkit/tests/support/mod.rs", "pub fn fixture() {}\n");
  for (const target of testkitTargets) {
    write(root, path.join("crates/koushi-core-testkit/tests", target), "#[test]\nfn moved() {}\n");
  }
  for (const target of coreLocalIntegrationTargets) {
    write(root, path.join("crates/koushi-core/tests", target), "#[test]\nfn local() {}\n");
  }
  write(
    root,
    ".github/workflows/ci.yml",
    "- run: cargo test -p koushi-core-testkit\n"
  );
  write(
    root,
    "crates/koushi-qa/Cargo.toml",
    '[package]\nname = "koushi-qa"\n[dependencies]\nkoushi-store = { path = "../koushi-store" }\n[features]\nqa-bin = ["koushi-store/test-hooks"]\n'
  );
  write(
    root,
    "crates/koushi-qa/src/lib.rs",
    "pub fn uses_store_probe() { koushi_store::resolved_credential_backend_is_file_dir(); }\n"
  );
  return root;
}

test("accepts the intended persistence and Core testkit boundaries", () => {
  assert.deepEqual(findLeafCrateBoundaryViolations(fixture()), []);
});

test("detects missing packages, dependency leaks, and retained Core crypto", () => {
  const root = fixture();
  fs.writeFileSync(
    path.join(root, "Cargo.toml"),
    '[workspace]\nmembers = ["crates/koushi-core"]\ndefault-members = ["crates/koushi-core"]\n'
  );
  fs.appendFileSync(path.join(root, "crates/koushi-store/Cargo.toml"), 'tokio = "1"\n');
  fs.appendFileSync(
    path.join(root, "crates/koushi-core/Cargo.toml"),
    'chacha20poly1305 = "0.10"\n'
  );
  fs.appendFileSync(
    path.join(root, "crates/koushi-core/src/lib.rs"),
    'use chacha20poly1305::ChaCha20Poly1305;\npub enum CredentialStoreBackend {}\n#[cfg(feature = "test-hooks")]\npub fn unit_test_hidden() {}\n'
  );

  const violations = findLeafCrateBoundaryViolations(root);
  assert(violations.includes("workspace member missing: crates/koushi-store"));
  assert(violations.includes("default workspace member missing: crates/koushi-store"));
  assert(violations.includes("koushi-store has forbidden dependency: tokio"));
  assert(violations.includes("koushi-core still depends on chacha20poly1305"));
  assert(violations.some((item) => item.includes("CredentialStoreBackend")));
  assert(violations.some((item) => item.includes("use chacha20poly1305")));
  assert(violations.some((item) => item.includes("Core test-hook cfg excludes unit tests")));
});

test("detects missing test-hook propagation and stale QA probe routing", () => {
  const root = fixture();
  fs.writeFileSync(
    path.join(root, "crates/koushi-core/Cargo.toml"),
    '[package]\nname = "koushi-core"\n[dependencies]\nkoushi-store = { path = "../koushi-store" }\n[features]\ntest-hooks = []\n'
  );
  fs.writeFileSync(
    path.join(root, "crates/koushi-qa/Cargo.toml"),
    '[package]\nname = "koushi-qa"\n[features]\nqa-bin = []\n'
  );
  fs.writeFileSync(
    path.join(root, "crates/koushi-qa/src/lib.rs"),
    "pub fn stale() { koushi_core::store::resolved_credential_backend_is_file_dir(); }\n"
  );

  const violations = findLeafCrateBoundaryViolations(root);
  assert(violations.includes("koushi-core test-hooks must enable koushi-store/test-hooks"));
  assert(violations.includes("koushi-qa must depend directly on koushi-store"));
  assert(violations.includes("koushi-qa qa-bin must enable koushi-store/test-hooks"));
  assert(violations.some((item) => item.includes("removed Core credential probe")));
});

test("detects direct and target-specific testkit build dependencies", () => {
  for (const header of [
    "[build-dependencies.helper]",
    "[target.'cfg(unix)'.build-dependencies]"
  ]) {
    const root = fixture();
    fs.writeFileSync(
      path.join(root, "crates/koushi-core-testkit/Cargo.toml"),
      `[package]\nname = "koushi-core-testkit"\npublish = false\n${header}\npath = "../helper"\n[dev-dependencies]\nkoushi-core = { path = "../koushi-core", features = ["test-hooks"] }\n`
    );
    assert(
      findLeafCrateBoundaryViolations(root).includes(
        "koushi-core-testkit must use dev-dependencies only"
      )
    );
  }
});

test("detects target-specific normal testkit dependencies", () => {
  const root = fixture();
  fs.writeFileSync(
    path.join(root, "crates/koushi-core-testkit/Cargo.toml"),
    '[package]\nname = "koushi-core-testkit"\npublish = false\n[target.\'cfg(unix)\'.dependencies]\nkoushi-qa = { path = "../koushi-qa" }\n[dev-dependencies]\nkoushi-core = { path = "../koushi-core", features = ["test-hooks"] }\n'
  );
  assert(
    findLeafCrateBoundaryViolations(root).includes(
      "koushi-core-testkit must use dev-dependencies only"
    )
  );
});

test("detects testkit default or production leakage, self-dependency, and missing targets", () => {
  const root = fixture();
  fs.appendFileSync(
    path.join(root, "crates/koushi-core/Cargo.toml"),
    'koushi-core = { path = ".", features = ["test-hooks"] }\nkoushi-core-testkit = { path = "../koushi-core-testkit" }\n'
  );
  fs.appendFileSync(
    path.join(root, "crates/koushi-qa/Cargo.toml"),
    'koushi-core-testkit = { path = "../koushi-core-testkit" }\n'
  );
  fs.writeFileSync(
    path.join(root, "crates/koushi-core-testkit/Cargo.toml"),
    '[package]\nname = "koushi-core-testkit"\npublish = true\n  [dependencies.koushi-qa] # forbidden normal edge\npath = "../koushi-qa"\n[dev-dependencies]\nkoushi-core = { path = "../koushi-core" }\n'
  );
  fs.rmSync(path.join(root, "crates/koushi-core-testkit/tests/support"), {
    recursive: true,
    force: true
  });
  fs.rmSync(path.join(root, "crates/koushi-core-testkit/tests", testkitTargets[0]));
  write(
    root,
    path.join("crates/koushi-core/tests", testkitTargets[1]),
    "#[test]\nfn duplicate() {}\n"
  );
  write(
    root,
    path.join("crates/koushi-core-testkit/tests", coreLocalIntegrationTargets[0]),
    "#[test]\nfn misplaced() {}\n"
  );
  fs.writeFileSync(path.join(root, ".github/workflows/ci.yml"), "jobs: {}\n");
  fs.writeFileSync(
    path.join(root, "Cargo.toml"),
    '[workspace]\nmembers = ["crates/koushi-store", "crates/koushi-core", "crates/koushi-core-testkit", "crates/koushi-qa"]\ndefault-members = ["crates/koushi-store", "crates/koushi-core", "crates/koushi-core-testkit"]\n'
  );

  const violations = findLeafCrateBoundaryViolations(root);
  assert(violations.includes("koushi-core-testkit must not be a default workspace member"));
  assert(violations.includes("koushi-core-testkit must be publish-disabled"));
  assert(violations.includes("koushi-core-testkit must use dev-dependencies only"));
  assert(violations.includes("koushi-core-testkit must enable koushi-core/test-hooks"));
  assert(violations.includes("koushi-core must not self-depend for test hooks"));
  assert(violations.includes("koushi-core must not depend on koushi-core-testkit"));
  assert(violations.includes("koushi-qa must not depend on koushi-core-testkit"));
  assert(violations.includes("shared Core integration support missing from koushi-core-testkit"));
  assert(violations.includes(`koushi-core-testkit target missing: ${testkitTargets[0]}`));
  assert(violations.includes(`moved integration target remains in koushi-core: ${testkitTargets[1]}`));
  assert(
    violations.includes(
      `Core-local integration target moved unnecessarily: ${coreLocalIntegrationTargets[0]}`
    )
  );
  assert(violations.includes("unexpected Rust integration target set in koushi-core"));
  assert(violations.includes("unexpected Rust integration target set in koushi-core-testkit"));
  assert(violations.includes("CI must run koushi-core-testkit explicitly"));
});
