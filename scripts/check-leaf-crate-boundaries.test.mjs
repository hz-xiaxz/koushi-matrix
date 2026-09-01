#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";

import { findLeafCrateBoundaryViolations } from "./check-leaf-crate-boundaries.mjs";

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
    '[workspace]\nmembers = ["crates/koushi-store", "crates/koushi-core", "crates/koushi-qa"]\ndefault-members = ["crates/koushi-store", "crates/koushi-core"]\n'
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

test("accepts the intended persistence leaf boundary", () => {
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
    "use chacha20poly1305::ChaCha20Poly1305;\npub enum CredentialStoreBackend {}\n"
  );

  const violations = findLeafCrateBoundaryViolations(root);
  assert(violations.includes("workspace member missing: crates/koushi-store"));
  assert(violations.includes("default workspace member missing: crates/koushi-store"));
  assert(violations.includes("koushi-store has forbidden dependency: tokio"));
  assert(violations.includes("koushi-core still depends on chacha20poly1305"));
  assert(violations.some((item) => item.includes("CredentialStoreBackend")));
  assert(violations.some((item) => item.includes("use chacha20poly1305")));
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
