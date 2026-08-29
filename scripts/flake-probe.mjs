#!/usr/bin/env node
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import {
  RESULT_SCHEMA_VERSION,
  renderSummaryMarkdown,
  summarizeRecords
} from "./summarize-flake-probe.mjs";

const repoRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const ATTEMPT_TIMEOUT_MS = 120_000;
const MAX_ATTEMPTS = 10;

// This is intentionally a closed list: attempts and output location are the
// only caller-controlled inputs; no arbitrary command or shell string is run.
export const PROBES = Object.freeze([
  {
    probe_id: "rust_commit_point",
    mode: "single-thread",
    command: "cargo",
    args: [
      "test",
      "-p",
      "koushi-core",
      "--lib",
      "runtime::tests::committed_room_cleanup_bypasses_a_saturated_account_mailbox",
      "--",
      "--exact",
      "--test-threads=1"
    ]
  },
  {
    probe_id: "rust_commit_point",
    mode: "default",
    command: "cargo",
    args: [
      "test",
      "-p",
      "koushi-core",
      "--lib",
      "runtime::tests::committed_room_cleanup_bypasses_a_saturated_account_mailbox",
      "--",
      "--exact"
    ]
  },
  {
    probe_id: "vitest_stale_live_edge",
    mode: "default",
    command: "npm",
    cwd: "apps/desktop",
    args: [
      "exec",
      "--",
      "vitest",
      "run",
      "src/components/TimelineView.scrollback.test.tsx",
      "-t",
      "drops a stale live-edge follow-up after user viewport input"
    ]
  },
  {
    probe_id: "vitest_unread_marker",
    mode: "default",
    command: "npm",
    cwd: "apps/desktop",
    args: [
      "exec",
      "--",
      "vitest",
      "run",
      "src/components/TimelineView.live-state.test.tsx",
      "-t",
      "hides the first-unread pill while keeping the unread marker and bottom pill"
    ]
  }
]);

function sha256Commit(cwd) {
  const result = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"]
  });
  if (result.status !== 0 || result.error) throw new Error("cannot resolve checked-out SHA");
  const sha = result.stdout.trim();
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("cannot resolve checked-out SHA");
  return sha;
}

function parseAttempts(value) {
  const attempts = Number(value ?? "10");
  if (!Number.isInteger(attempts) || attempts < 1 || attempts > MAX_ATTEMPTS) {
    throw new Error(`attempts must be an integer from 1 to ${MAX_ATTEMPTS}`);
  }
  return attempts;
}

function parseArgs(args) {
  const value = (name) => {
    const prefix = `${name}=`;
    const inline = args.find((arg) => arg.startsWith(prefix));
    if (inline) return inline.slice(prefix.length);
    const index = args.indexOf(name);
    return index >= 0 ? args[index + 1] : undefined;
  };
  const known = new Set(["--attempts", "--sha", "--output-dir"]);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (known.has(arg)) index += 1;
    else if (!known.has(arg.split("=", 1)[0])) throw new Error("unknown probe argument");
  }
  const outputDir = value("--output-dir") ?? "artifacts/issue-738-flake-probe";
  const expectedSha = value("--sha");
  if (!outputDir || outputDir.startsWith("-")) throw new Error("invalid output directory");
  if (expectedSha !== undefined && !/^[0-9a-f]{40}$/.test(expectedSha)) {
    throw new Error("invalid SHA");
  }
  return {
    attempts: parseAttempts(value("--attempts")),
    expectedSha,
    outputDir: resolve(repoRoot, outputDir)
  };
}

function fixedFailureSignature(result) {
  if (result.error?.code === "ETIMEDOUT" || result.signal === "SIGTERM") return "timeout";
  if (result.error) return "spawn_error";
  if (result.signal) return "signal";
  return "exit_nonzero";
}

function safeEnvironment() {
  return Object.fromEntries(
    Object.entries(process.env).filter(
      ([name]) => !/(?:TOKEN|PASSWORD|SECRET|PRIVATE|CREDENTIAL|API[_-]?KEY)/i.test(name)
    )
  );
}

export function runAttempt(probe, sha, attempt, { cwd = repoRoot } = {}) {
  const started = new Date().toISOString();
  const start = performance.now();
  const result = spawnSync(probe.command, probe.args, {
    cwd: probe.cwd ? resolve(cwd, probe.cwd) : cwd,
    env: safeEnvironment(),
    encoding: "utf8",
    stdio: ["ignore", "ignore", "ignore"],
    timeout: ATTEMPT_TIMEOUT_MS,
    killSignal: "SIGTERM"
  });
  const durationMs = Math.max(0, Math.round(performance.now() - start));
  const passed = result.status === 0 && !result.error && !result.signal;
  return {
    schema_version: RESULT_SCHEMA_VERSION,
    sha,
    probe_id: probe.probe_id,
    mode: probe.mode,
    attempt,
    result: passed ? "passed" : "failed",
    duration_ms: durationMs,
    failure_signature: passed ? "none" : fixedFailureSignature(result),
    recorded_at: started
  };
}

function xmlEscape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

export function renderJunit(records) {
  const failures = records.filter((record) => record.result === "failed").length;
  const cases = records.map((record) => {
    const name = `${record.probe_id}/${record.mode}/attempt-${record.attempt}`;
    const attributes = `classname="issue-738-flake-probe" name="${xmlEscape(name)}" time="${(record.duration_ms / 1000).toFixed(3)}"`;
    return record.result === "failed"
      ? `  <testcase ${attributes}><failure message="${xmlEscape(record.failure_signature)}" /></testcase>`
      : `  <testcase ${attributes} />`;
  });
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    `<testsuite name="issue-738-flake-probe" tests="${records.length}" failures="${failures}">`,
    ...cases,
    "</testsuite>",
    ""
  ].join("\n");
}

function writeArtifacts(outputDir, sha, records) {
  mkdirSync(outputDir, { recursive: true });
  const summary = summarizeRecords(records, { expectedSha: sha });
  writeFileSync(
    join(outputDir, "flake-probe-results.json"),
    `${JSON.stringify({
      schema_version: RESULT_SCHEMA_VERSION,
      sha,
      attempts: records,
      generated_at: new Date().toISOString()
    }, null, 2)}\n`
  );
  writeFileSync(join(outputDir, "flake-probe-results.junit.xml"), renderJunit(records));
  writeFileSync(join(outputDir, "flake-probe-summary.md"), renderSummaryMarkdown(summary));
  return summary;
}

export function main(args = process.argv.slice(2)) {
  const options = parseArgs(args);
  const sha = sha256Commit(repoRoot);
  if (options.expectedSha !== undefined && options.expectedSha !== sha) {
    throw new Error("checked-out SHA does not match requested SHA");
  }
  const records = [];
  for (let attempt = 1; attempt <= options.attempts; attempt += 1) {
    for (const probe of PROBES) records.push(runAttempt(probe, sha, attempt));
  }
  if (sha256Commit(repoRoot) !== sha) throw new Error("checked-out SHA changed during probe");
  const summary = writeArtifacts(options.outputDir, sha, records);
  process.stdout.write(renderSummaryMarkdown(summary));
  return summary.failures === 0 ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`flake-probe: ${error.message}`);
    process.exitCode = 2;
  }
}
