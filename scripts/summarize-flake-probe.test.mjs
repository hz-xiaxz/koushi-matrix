import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PROBES, renderJunit, runAttempt } from "./flake-probe.mjs";
import {
  RESULT_SCHEMA_VERSION,
  main as summarizeMain,
  parseResultDocument,
  parseResultFile,
  summarizeRecords
} from "./summarize-flake-probe.mjs";

const attempt = (overrides = {}) => ({
  schema_version: RESULT_SCHEMA_VERSION,
  sha: "a".repeat(40),
  probe_id: "rust_commit_point",
  mode: "single-thread",
  attempt: 1,
  result: "passed",
  duration_ms: 12,
  failure_signature: "none",
  recorded_at: "2026-08-30T00:00:00.000Z",
  ...overrides
});

test("summarizes passed and failed attempts over a reproducible date window", () => {
  const records = [
    attempt(),
    attempt({
      attempt: 2,
      result: "failed",
      failure_signature: "exit_nonzero",
      duration_ms: 20,
      recorded_at: "2026-08-31T00:00:00.000Z"
    })
  ];

  assert.deepEqual(summarizeRecords(records, { expectedSha: "a".repeat(40) }), {
    schema_version: RESULT_SCHEMA_VERSION,
    sha: "a".repeat(40),
    attempts: 2,
    failures: 1,
    failure_rate: 0.5,
    date_window: {
      start: "2026-08-30T00:00:00.000Z",
      end: "2026-08-31T00:00:00.000Z",
      days: 1
    },
    seven_day_interpretation: "pending_insufficient_window",
    ci_ten_run_interpretation: "pending",
    threshold_breached: false
  });
});

test("parses the generated result envelope", () => {
  const parsed = parseResultDocument({
    schema_version: RESULT_SCHEMA_VERSION,
    sha: "b".repeat(40),
    attempts: [attempt({ sha: "b".repeat(40) })]
  });

  assert.equal(parsed.length, 1);
  assert.equal(parsed[0].sha, "b".repeat(40));
});

test("rejects malformed result data", () => {
  assert.throws(
    () => parseResultDocument({ schema_version: RESULT_SCHEMA_VERSION, attempts: [] }),
    /malformed result data/
  );
  assert.throws(
    () => parseResultDocument({
      schema_version: RESULT_SCHEMA_VERSION,
      sha: "c".repeat(40),
      attempts: [attempt({ sha: "c".repeat(40), duration_ms: -1 })]
    }),
    /malformed result data/
  );
});

test("rejects mixed SHAs when unchanged SHA validation is requested", () => {
  const records = [attempt(), attempt({ sha: "d".repeat(40), attempt: 2 })];

  assert.throws(
    () => summarizeRecords(records, { requireUnchangedSha: true }),
    /mixed SHA/
  );
  assert.throws(
    () => summarizeRecords(records, { expectedSha: "a".repeat(40) }),
    /SHA mismatch/
  );
});

test("accepts repeated attempt numbers from separate workflow artifacts", () => {
  const summary = summarizeRecords([
    attempt(),
    attempt({ recorded_at: "2026-08-31T00:00:00.000Z" })
  ]);

  assert.equal(summary.attempts, 2);
});

test("marks equality at a strict failure-rate threshold as a breach", () => {
  const records = Array.from({ length: 100 }, (_, index) =>
    attempt({
      attempt: index + 1,
      result: index === 99 ? "failed" : "passed",
      failure_signature: index === 99 ? "timeout" : "none",
      recorded_at: new Date(Date.UTC(2026, 7, 30, 0, 0, 0, index)).toISOString()
    })
  );

  assert.equal(summarizeRecords(records, { maxFailureRate: 0.01 }).threshold_breached, true);
});

test("probe whitelist names non-vacuous exact Rust and Vitest tests", () => {
  const rustProbes = PROBES.filter((probe) => probe.probe_id === "rust_commit_point");
  const vitestProbes = PROBES.filter((probe) => probe.command === "npm");

  const runtimeSource = readFileSync(
    new URL("../crates/koushi-core/src/runtime.rs", import.meta.url),
    "utf8"
  );
  assert.match(
    runtimeSource,
    /async fn committed_room_cleanup_bypasses_a_saturated_account_mailbox\(\)/
  );

  assert.equal(rustProbes.length, 2);
  for (const probe of rustProbes) {
    assert.ok(
      probe.args.includes(
        "runtime::tests::committed_room_cleanup_bypasses_a_saturated_account_mailbox"
      )
    );
    assert.ok(probe.args.includes("--exact"));
  }
  assert.equal(vitestProbes.length, 2);
  assert.ok(vitestProbes.every((probe) => probe.args.includes("-t")));
  assert.ok(vitestProbes.some((probe) => probe.args.includes(
    "drops a stale live-edge follow-up after user viewport input"
  )));
  assert.ok(vitestProbes.some((probe) => probe.args.includes(
    "hides the first-unread pill while keeping the unread marker and bottom pill"
  )));
});

test("round-trips result text through the CLI file and JSON output path", () => {
  const directory = mkdtempSync(join(tmpdir(), "flake-summary-"));
  const input = join(directory, "input.json");
  const output = join(directory, "summary.json");
  const document = {
    schema_version: RESULT_SCHEMA_VERSION,
    sha: "a".repeat(40),
    attempts: [attempt()]
  };
  writeFileSync(input, JSON.stringify(document));

  assert.equal(parseResultFile(readFileSync(input, "utf8")).length, 1);
  assert.equal(summarizeMain([input, "--format", "json", "--output", output]), 0);
  assert.equal(JSON.parse(readFileSync(output, "utf8")).attempts, 1);
  assert.throws(() => summarizeMain([input, "--sha", "not-a-sha"]), /invalid SHA/);
  rmSync(directory, { recursive: true });
});

test("rejects duplicate attempt identities", () => {
  assert.throws(() => summarizeRecords([attempt(), attempt()]), /malformed result data/);
});

test("interprets a complete seven-day window below one percent", () => {
  const records = Array.from({ length: 101 }, (_, index) =>
    attempt({
      attempt: index + 1,
      result: index === 100 ? "failed" : "passed",
      failure_signature: index === 100 ? "exit_nonzero" : "none",
      recorded_at: new Date(Date.UTC(2026, 7, 30) + index * (7 * 86_400_000 / 100)).toISOString()
    })
  );

  assert.equal(
    summarizeRecords(records).seven_day_interpretation,
    "measured_below_1_percent"
  );
});

test("renders failure-safe JUnit and classifies a nonzero child exit", () => {
  const failed = attempt({ result: "failed", failure_signature: "exit_nonzero" });
  const junit = renderJunit([failed]);
  assert.match(junit, /tests="1" failures="1"/);
  assert.match(junit, /<failure message="exit_nonzero"/);

  const result = runAttempt(
    { probe_id: "test", mode: "default", command: process.execPath, args: ["-e", "process.exit(3)"] },
    "e".repeat(40),
    1
  );
  assert.equal(result.result, "failed");
  assert.equal(result.failure_signature, "exit_nonzero");
});
