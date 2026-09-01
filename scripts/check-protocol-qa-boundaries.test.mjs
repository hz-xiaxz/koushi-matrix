#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";

import { findProtocolQaBoundaryViolations } from "./check-protocol-qa-boundaries.mjs";

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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "koushi-protocol-boundary-"));
  roots.push(root);
  write(
    root,
    "Cargo.toml",
    '[workspace]\nmembers = ["crates/koushi-protocol", "crates/koushi-core", "crates/koushi-qa"]\n'
  );
  write(
    root,
    "crates/koushi-protocol/Cargo.toml",
    '[package]\nname = "koushi-protocol"\n[dependencies]\nserde = "1"\nkoushi-state = { path = "../koushi-state" }\n'
  );
  write(
    root,
    "crates/koushi-protocol/src/lib.rs",
    "pub enum CoreCommand {}\npub enum CoreEvent {}\npub struct RequestId;\npub enum CoreFailure {}\npub struct StateDelta;\n"
  );
  write(root, "crates/koushi-core/Cargo.toml", '[package]\nname = "koushi-core"\n');
  write(root, "crates/koushi-core/src/lib.rs", "pub struct CoreRuntime;\n");
  write(
    root,
    "crates/koushi-qa/Cargo.toml",
    '[package]\nname = "koushi-qa"\n[[bin]]\nname = "headless-core-qa"\nrequired-features = ["qa-bin"]\n[[bin]]\nname = "real-homeserver-qa"\nrequired-features = ["qa-bin"]\n'
  );
  return root;
}

test("accepts the intended protocol and QA ownership", () => {
  assert.deepEqual(findProtocolQaBoundaryViolations(fixture()), []);
});

test("detects forbidden protocol dependencies and runtime tokens", () => {
  const root = fixture();
  fs.appendFileSync(
    path.join(root, "crates/koushi-protocol/Cargo.toml"),
    'tokio = "1"\nkoushi-key = { path = "../koushi-key" }\n'
  );
  fs.appendFileSync(
    path.join(root, "crates/koushi-protocol/src/lib.rs"),
    "pub fn bad(_: std::path::PathBuf) { tokio::spawn(async {}); }\n"
  );
  const violations = findProtocolQaBoundaryViolations(root);
  assert(violations.some((item) => item.includes("forbidden dependency: tokio")));
  assert(violations.some((item) => item.includes("forbidden dependency: koushi-key")));
  assert(violations.some((item) => item.includes("forbidden token tokio::")));
  assert(violations.some((item) => item.includes("forbidden token PathBuf")));
});

test("detects DTOs, QA trees, and Tauri URI minting left in Core", () => {
  const root = fixture();
  fs.appendFileSync(
    path.join(root, "crates/koushi-core/src/lib.rs"),
    'pub enum CoreEvent {}\nconst URI: &str = "koushi-thumbnail://localhost/";\n'
  );
  write(root, "crates/koushi-core/src/bin/headless-core-qa.rs", "fn main() {}\n");
  const violations = findProtocolQaBoundaryViolations(root);
  assert(violations.some((item) => item.includes("QA source remains in koushi-core")));
  assert(violations.some((item) => item.includes("Core mints Tauri thumbnail URI")));
  assert(violations.some((item) => item.includes("protocol definition remains in Core")));
});

test("detects missing workspace packages and preserved QA binaries", () => {
  const root = fixture();
  fs.writeFileSync(path.join(root, "Cargo.toml"), '[workspace]\nmembers = ["crates/koushi-core"]\n');
  fs.rmSync(path.join(root, "crates/koushi-qa"), { recursive: true });
  const violations = findProtocolQaBoundaryViolations(root);
  assert(violations.includes("workspace member missing: crates/koushi-protocol"));
  assert(violations.includes("workspace member missing: crates/koushi-qa"));
  assert(violations.includes("missing crates/koushi-qa/Cargo.toml"));
});
