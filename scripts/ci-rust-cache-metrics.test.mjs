import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("./ci-rust-cache-metrics.mjs", import.meta.url));

test("Rust cache metrics records compilation, test, and target-size measurements", () => {
  const root = mkdtempSync(join(tmpdir(), "koushi-ci-metrics-"));
  const target = join(root, "target-ci");
  const log = join(root, "cargo.log");
  mkdirSync(target, { recursive: true });
  writeFileSync(join(target, "artifact.rlib"), "artifact");
  writeFileSync(
    log,
    [
      "   Compiling matrix-sdk v0.18.0",
      "    Checking koushi-core v0.1.0",
      "    Finished `ci` profile [unoptimized] target(s) in 42.00s",
      "test result: ok. 12 passed; 0 failed; 1 ignored"
    ].join("\n")
  );
  const result = spawnSync(process.execPath, [
    script,
    `--log=${log}`,
    `--target-dir=${target}`,
    "--mode=warm",
    "--cache-hit=true",
    "--elapsed-seconds=42"
  ], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_metric_compiling_lines=1/);
  assert.match(result.stdout, /rust_metric_checking_lines=1/);
  assert.match(result.stdout, /rust_metric_test_counts=\{"passed":12,"failed":0,"ignored":1\}/);
  assert.match(result.stdout, /rust_metric_target_bytes=8/);
});

test("Rust cache metrics ignores ANSI color and counts vendored SDK recompilation", () => {
  const root = mkdtempSync(join(tmpdir(), "koushi-ci-metrics-"));
  const target = join(root, "target-ci");
  const log = join(root, "cargo.log");
  mkdirSync(target, { recursive: true });
  writeFileSync(
    log,
    [
      "\u001b[1m\u001b[92m   Compiling\u001b[0m matrix-sdk-common v0.18.0 (/work/vendor/matrix-rust-sdk/crates/matrix-sdk-common)",
      "\u001b[1m\u001b[92m   Compiling\u001b[0m koushi-core v0.1.0 (/work/crates/koushi-core)",
      "   Compiling tokio v1.0.0",
      "test result: ok. 3 passed; 0 failed; 0 ignored"
    ].join("\n")
  );
  const result = spawnSync(process.execPath, [script, `--log=${log}`, `--target-dir=${target}`, "--mode=rust"], {
    encoding: "utf8"
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_metric_compiling_lines=3/);
  assert.match(result.stdout, /rust_metric_vendored_sdk_compiling_lines=1/);
});

test("Rust cache metrics accepts space-separated options as the benchmark workflow passes them", () => {
  const root = mkdtempSync(join(tmpdir(), "koushi-ci-metrics-"));
  const log = join(root, "cargo.log");
  writeFileSync(log, "test result: ok. 2 passed; 0 failed; 0 ignored\n");
  const result = spawnSync(
    process.execPath,
    [script, "--log", log, "--target-dir", join(root, "target-ci"), "--mode", "cold", "--cache-hit", "false", "--elapsed-seconds", "7"],
    { encoding: "utf8" }
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /rust_metric_mode=cold/);
  assert.match(result.stdout, /rust_metric_elapsed_seconds=7/);
});
