import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("./ci-rust-cache-report.mjs", import.meta.url));

test("cache report accepts a cold target without claiming a cache hit", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  const result = spawnSync(process.execPath, [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--sdk-cache-hit=false"], {
    encoding: "utf8"
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_cache_sdk_cache_hit=false/);
  assert.match(result.stdout, /rust_cache_sdk_artifacts=0/);
});

test("cache report rejects a claimed SDK hit without representative artifacts", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  mkdirSync(join(target, "target-ci", "ci", ".fingerprint"), { recursive: true });
  const result = spawnSync(process.execPath, [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--sdk-cache-hit=true"], {
    encoding: "utf8"
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /no ci-profile artifacts were restored/);
});

test("cache report counts SDK fingerprints and artifacts in the selected profile", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  const profile = join(target, "target-ci", "ci");
  mkdirSync(join(profile, ".fingerprint", "matrix-sdk-example"), { recursive: true });
  mkdirSync(join(profile, "deps"), { recursive: true });
  writeFileSync(join(profile, ".fingerprint", "matrix-sdk-example", "lib-matrix-sdk.json"), "{}");
  writeFileSync(join(profile, "deps", "libmatrix_sdk_example.rlib"), "artifact");
  const result = spawnSync(process.execPath, [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--sdk-cache-hit=true"], {
    encoding: "utf8"
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_cache_sdk_fingerprints=1/);
  assert.match(result.stdout, /rust_cache_sdk_artifacts=1/);
});

test("cache report accepts space-separated options", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  const profile = join(target, "target-release", "release");
  mkdirSync(join(profile, ".fingerprint", "matrix-sdk-example"), { recursive: true });
  mkdirSync(join(profile, "deps"), { recursive: true });
  writeFileSync(join(profile, ".fingerprint", "matrix-sdk-example", "lib-matrix-sdk.json"), "{}");
  writeFileSync(join(profile, "deps", "libmatrix_sdk_example.rlib"), "artifact");
  const result = spawnSync(
    process.execPath,
    [script, "--target-dir", join(target, "target-release"), "--profile", "release", "--sdk-cache-hit", "true", "--label", "release"],
    { encoding: "utf8" }
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_cache_profile=release/);
  assert.match(result.stdout, /rust_cache_sdk_cache_hit=true/);
  assert.match(result.stdout, /rust_cache_sdk_fingerprints=1/);
  assert.match(result.stdout, /rust_cache_sdk_artifacts=1/);
});

test("cache report rejects a claimed rust-cache hit without representative dependency artifacts", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  mkdirSync(join(target, "target-ci", "ci", "deps"), { recursive: true });
  writeFileSync(join(target, "target-ci", "ci", "deps", "libserde-0123456789abcdef.rlib"), "artifact");
  writeFileSync(join(target, "target-ci", "ci", "deps", "libruma-0123456789abcdef.rlib"), "artifact");
  const result = spawnSync(
    process.execPath,
    [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--rust-cache-hit=true", "--sdk-cache-hit=false"],
    { encoding: "utf8" }
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /rust-cache reported a hit but representative ci-profile dependency artifacts are missing: tokio/);
});

test("cache report accepts a rust-cache hit with representative dependency artifacts", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  const deps = join(target, "target-ci", "ci", "deps");
  mkdirSync(deps, { recursive: true });
  for (const name of ["libtokio-0123456789abcdef.rlib", "libserde-0123456789abcdef.rlib", "libruma-0123456789abcdef.rlib"]) {
    writeFileSync(join(deps, name), "artifact");
  }
  const result = spawnSync(
    process.execPath,
    [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--rust-cache-hit=true", "--sdk-cache-hit=false"],
    { encoding: "utf8" }
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_cache_rust_cache_hit=true/);
  assert.match(result.stdout, /rust_cache_dependency_artifacts_missing=none/);
});

test("cache report does not require dependency artifacts on a rust-cache miss", () => {
  const target = mkdtempSync(join(tmpdir(), "koushi-ci-cache-"));
  const result = spawnSync(
    process.execPath,
    [script, `--target-dir=${join(target, "target-ci")}`, "--profile=ci", "--rust-cache-hit=false"],
    { encoding: "utf8" }
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_cache_dependency_artifacts_missing=tokio,serde,ruma/);
});
