#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const RESULT_SCHEMA_VERSION = 1;
const SHA_RE = /^[0-9a-f]{40}$/;
const ID_RE = /^[A-Za-z0-9._-]+$/;
const RESULTS = new Set(["passed", "failed"]);
const FAILURE_SIGNATURES = new Set([
  "none",
  "exit_nonzero",
  "timeout",
  "signal",
  "spawn_error"
]);

function invalid() {
  return new Error("malformed result data");
}

function assertSha(value) {
  if (typeof value !== "string" || !SHA_RE.test(value)) throw invalid();
}

function assertAttempt(record) {
  if (!record || typeof record !== "object" || Array.isArray(record)) throw invalid();
  if (record.schema_version !== RESULT_SCHEMA_VERSION) throw invalid();
  assertSha(record.sha);
  if (
    typeof record.probe_id !== "string" ||
    !ID_RE.test(record.probe_id) ||
    typeof record.mode !== "string" ||
    !ID_RE.test(record.mode) ||
    !Number.isSafeInteger(record.attempt) ||
    record.attempt < 1 ||
    !RESULTS.has(record.result) ||
    !Number.isSafeInteger(record.duration_ms) ||
    record.duration_ms < 0 ||
    !FAILURE_SIGNATURES.has(record.failure_signature) ||
    typeof record.recorded_at !== "string" ||
    !Number.isFinite(Date.parse(record.recorded_at))
  ) {
    throw invalid();
  }
  if (record.result === "passed" && record.failure_signature !== "none") throw invalid();
  if (record.result === "failed" && record.failure_signature === "none") throw invalid();
}

export function parseResultDocument(document) {
  if (
    !document ||
    typeof document !== "object" ||
    Array.isArray(document) ||
    document.schema_version !== RESULT_SCHEMA_VERSION ||
    !Array.isArray(document.attempts)
  ) {
    throw invalid();
  }
  assertSha(document.sha);
  const records = document.attempts.map((record) => {
    assertAttempt(record);
    if (record.sha !== document.sha) throw invalid();
    return record;
  });
  if (records.length === 0) throw invalid();
  return records;
}

export function parseResultFile(text) {
  try {
    return parseResultDocument(JSON.parse(text));
  } catch (error) {
    if (error?.message === "malformed result data") throw error;
    throw invalid();
  }
}

function validateRecords(records) {
  if (!Array.isArray(records) || records.length === 0) throw invalid();
  const seen = new Set();
  for (const record of records) {
    assertAttempt(record);
    // Attempt numbers restart in each scheduled/manual workflow run, so the
    // timestamp is part of the cross-artifact identity.
    const key = `${record.sha}:${record.probe_id}:${record.mode}:${record.attempt}:${record.recorded_at}`;
    if (seen.has(key)) throw invalid();
    seen.add(key);
  }
}

function validateRate(value) {
  if (value === undefined) return;
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) {
    throw new Error("invalid failure-rate threshold");
  }
}

export function summarizeRecords(
  records,
  { expectedSha, requireUnchangedSha = false, maxFailureRate } = {}
) {
  validateRecords(records);
  validateRate(maxFailureRate);
  if (expectedSha !== undefined) assertSha(expectedSha);

  const shas = [...new Set(records.map((record) => record.sha))].sort();
  if (requireUnchangedSha && shas.length !== 1) throw new Error("mixed SHA");
  if (expectedSha !== undefined && records.some((record) => record.sha !== expectedSha)) {
    throw new Error("SHA mismatch");
  }

  const ordered = [...records].sort((left, right) =>
    Date.parse(left.recorded_at) - Date.parse(right.recorded_at) ||
    left.sha.localeCompare(right.sha) ||
    left.probe_id.localeCompare(right.probe_id) ||
    left.mode.localeCompare(right.mode) ||
    left.attempt - right.attempt
  );
  const failures = ordered.filter((record) => record.result === "failed").length;
  const start = ordered[0].recorded_at;
  const end = ordered.at(-1).recorded_at;
  const days = (Date.parse(end) - Date.parse(start)) / 86_400_000;
  const failureRate = failures / ordered.length;
  // Acceptance is strictly below 1%, so equality at a configured threshold
  // is a breach rather than a pass.
  const thresholdBreached = maxFailureRate !== undefined && failureRate >= maxFailureRate;
  const sevenDayInterpretation = days >= 7
    ? failureRate < 0.01
      ? "measured_below_1_percent"
      : "measured_at_or_above_1_percent"
    : "pending_insufficient_window";

  return {
    schema_version: RESULT_SCHEMA_VERSION,
    sha: shas.length === 1 ? shas[0] : "mixed",
    attempts: ordered.length,
    failures,
    failure_rate: failureRate,
    date_window: { start, end, days },
    seven_day_interpretation: sevenDayInterpretation,
    ci_ten_run_interpretation: "pending",
    threshold_breached: Boolean(thresholdBreached)
  };
}

export function renderSummaryMarkdown(summary) {
  const rate = (summary.failure_rate * 100).toFixed(2);
  const sevenDay = summary.seven_day_interpretation === "pending_insufficient_window"
    ? "pending (less than seven days of observations)"
    : summary.seven_day_interpretation === "measured_below_1_percent"
      ? "measured below 1% for this probe window; ten unchanged-SHA full CI runs remain separate"
      : "measured at or above 1% for this probe window";
  return [
    "# Issue #738 flake probe",
    "",
    `- SHA: \`${summary.sha}\``,
    `- Attempts: ${summary.attempts}`,
    `- Failures: ${summary.failures} (${rate}%)`,
    `- Date window: ${summary.date_window.start} to ${summary.date_window.end} (${summary.date_window.days} days)`,
    `- Seven-day interpretation: ${sevenDay}`,
    "- Ten unchanged-SHA full CI runs: pending (this probe does not measure rerun-free CI runs)",
    "",
    "Each row is one executed attempt. A workflow rerun is a separate GitHub run, not a successful replacement for a failed attempt.",
    ""
  ].join("\n");
}

function optionValue(args, name) {
  const prefix = `${name}=`;
  const inline = args.find((arg) => arg.startsWith(prefix));
  if (inline) return inline.slice(prefix.length);
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
}

function parseCli(args) {
  const files = [];
  const valueOptions = new Set(["--sha", "--max-failure-rate", "--format", "--output"]);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const name = arg.split("=", 1)[0];
    if (valueOptions.has(name) && !arg.includes("=")) {
      if (index + 1 >= args.length) throw new Error("invalid summarizer arguments");
      index += 1;
    } else if (arg === "--require-unchanged-sha") {
      continue;
    } else if (arg.startsWith("--")) {
      throw new Error("invalid summarizer arguments");
    } else {
      files.push(arg);
    }
  }
  const expectedSha = optionValue(args, "--sha");
  if (expectedSha !== undefined && !SHA_RE.test(expectedSha)) {
    throw new Error("invalid SHA");
  }
  const maxValue = optionValue(args, "--max-failure-rate");
  const format = optionValue(args, "--format") ?? "markdown";
  if (files.length === 0 || (format !== "markdown" && format !== "json")) {
    throw new Error("invalid summarizer arguments");
  }
  const maxFailureRate = maxValue === undefined ? undefined : Number(maxValue);
  if (maxValue !== undefined && !Number.isFinite(maxFailureRate)) {
    throw new Error("invalid failure-rate threshold");
  }
  return {
    files,
    expectedSha,
    requireUnchangedSha: args.includes("--require-unchanged-sha"),
    maxFailureRate,
    format,
    output: optionValue(args, "--output")
  };
}

export function main(args = process.argv.slice(2)) {
  const options = parseCli(args);
  const records = options.files.flatMap((file) =>
    parseResultFile(readFileSync(file, "utf8"))
  );
  const summary = summarizeRecords(records, options);
  const output = options.format === "json"
    ? `${JSON.stringify(summary, null, 2)}\n`
    : renderSummaryMarkdown(summary);
  if (options.output) writeFileSync(options.output, output);
  else process.stdout.write(output);
  return summary.threshold_breached ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch {
    console.error("summarize-flake-probe: invalid result data or arguments");
    process.exitCode = 2;
  }
}
