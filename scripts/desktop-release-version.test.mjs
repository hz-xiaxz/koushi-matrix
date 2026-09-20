#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = resolve(dirname(fileURLToPath(import.meta.url)), "desktop-release-version.mjs");

/** Write the four synchronized release files into a throwaway root. */
function fixtureRoot({ packageVersion, tauriVersion, cargoVersion, lockVersion }) {
  const root = mkdtempSync(join(tmpdir(), "koushi-release-version-"));
  mkdirSync(join(root, "apps/desktop/src-tauri"), { recursive: true });
  writeFileSync(
    join(root, "apps/desktop/package.json"),
    JSON.stringify({ name: "koushi-desktop", version: packageVersion })
  );
  writeFileSync(
    join(root, "apps/desktop/src-tauri/tauri.conf.json"),
    JSON.stringify({ productName: "Koushi", version: tauriVersion })
  );
  writeFileSync(
    join(root, "apps/desktop/src-tauri/Cargo.toml"),
    `[package]\nname = "koushi-desktop"\nversion = "${cargoVersion}"\nedition = "2024"\n\n[dependencies]\nserde = "1"\n`
  );
  writeFileSync(
    join(root, "Cargo.lock"),
    `version = 4\n\n[[package]]\nname = "koushi-state"\nversion = "0.1.0"\n\n[[package]]\nname = "koushi-desktop"\nversion = "${lockVersion}"\ndependencies = [\n "serde",\n]\n`
  );
  return root;
}

function run(root) {
  return spawnSync(process.execPath, [script, "--root", root], { encoding: "utf8" });
}

test("accepts a release whose lockfile matches the three manifests", () => {
  const result = run(
    fixtureRoot({
      packageVersion: "1.2.3",
      tauriVersion: "1.2.3",
      cargoVersion: "1.2.3",
      lockVersion: "1.2.3"
    })
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^version=1\.2\.3$/m);
  assert.match(result.stdout, /^tag=v1\.2\.3$/m);
});

test("rejects a release that bumped the manifests but left Cargo.lock behind", () => {
  const result = run(
    fixtureRoot({
      packageVersion: "1.2.3",
      tauriVersion: "1.2.3",
      cargoVersion: "1.2.3",
      lockVersion: "1.2.2"
    })
  );
  assert.equal(result.status, 1);
  assert.match(result.stderr, /release versions do not match \(current\)/);
  assert.match(result.stderr, /lock=1\.2\.2/);
  assert.match(result.stderr, /refresh Cargo\.lock/);
});

test("still rejects a manifest that disagrees with the others", () => {
  const result = run(
    fixtureRoot({
      packageVersion: "1.2.3",
      tauriVersion: "1.2.4",
      cargoVersion: "1.2.3",
      lockVersion: "1.2.3"
    })
  );
  assert.equal(result.status, 1);
  assert.match(result.stderr, /tauri=1\.2\.4/);
});

test("reports a lockfile with no koushi-desktop package", () => {
  const root = fixtureRoot({
    packageVersion: "1.2.3",
    tauriVersion: "1.2.3",
    cargoVersion: "1.2.3",
    lockVersion: "1.2.3"
  });
  writeFileSync(join(root, "Cargo.lock"), 'version = 4\n\n[[package]]\nname = "serde"\nversion = "1"\n');
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Cargo\.lock has no koushi-desktop package version/);
});
