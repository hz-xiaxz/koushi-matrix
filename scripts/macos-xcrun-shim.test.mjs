import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const shim = resolve(dirname(fileURLToPath(import.meta.url)), "macos", "xcrun");

// A stand-in for /usr/bin/xcrun that fails the first `failures` calls with
// exit 65 and records every argument list it receives.
function fakeXcrun(failures) {
  const dir = mkdtempSync(join(tmpdir(), "koushi-xcrun-shim-"));
  const counter = join(dir, "count");
  const calls = join(dir, "calls");
  writeFileSync(counter, "0");
  writeFileSync(calls, "");
  const fake = join(dir, "xcrun");
  writeFileSync(
    fake,
    `#!/usr/bin/env bash
n=$(( $(cat "${counter}") + 1 ))
echo "$n" > "${counter}"
echo "$*" >> "${calls}"
if (( n <= ${failures} )); then echo "Koushi.app does not have a ticket stapled to it." >&2; exit 65; fi
echo "The staple and validate action worked!"
`,
    { mode: 0o755 }
  );
  return { fake, calls, log: join(dir, "stapler.log") };
}

function runShim(args, { fake, log }, delays = "0 0 0") {
  return spawnSync(shim, args, {
    encoding: "utf8",
    env: { ...process.env, KOUSHI_REAL_XCRUN: fake, KOUSHI_STAPLE_LOG: log, KOUSHI_STAPLE_RETRY_DELAYS: delays }
  });
}

test("a staple that fails while the ticket propagates is retried until it succeeds", () => {
  const fixture = fakeXcrun(2);
  const result = runShim(["stapler", "staple", "-v", "Koushi.app"], fixture);
  assert.equal(result.status, 0);
  assert.equal(readFileSync(fixture.calls, "utf8").trim().split("\n").length, 3);
  const log = readFileSync(fixture.log, "utf8");
  assert.match(log, /attempt 1 exit 65/);
  assert.match(log, /does not have a ticket stapled/);
  assert.match(log, /attempt 3 exit 0/);
});

test("a staple that never succeeds exits nonzero after the bounded attempts and says so", () => {
  const fixture = fakeXcrun(99);
  const result = runShim(["stapler", "staple", "-v", "Koushi.app"], fixture, "0 0");
  assert.equal(result.status, 65);
  assert.equal(readFileSync(fixture.calls, "utf8").trim().split("\n").length, 3);
  assert.match(readFileSync(fixture.log, "utf8"), /failed after 3 attempts/);
});

test("stapler validate is retried the same way", () => {
  const fixture = fakeXcrun(1);
  const result = runShim(["stapler", "validate", "Koushi.app"], fixture);
  assert.equal(result.status, 0);
  assert.equal(readFileSync(fixture.calls, "utf8").trim().split("\n").length, 2);
});

test("other xcrun commands pass through once, unchanged and unretried", () => {
  const fixture = fakeXcrun(1);
  const result = runShim(["notarytool", "submit", "Koushi.zip", "--wait"], fixture);
  assert.equal(result.status, 65);
  assert.equal(readFileSync(fixture.calls, "utf8"), "notarytool submit Koushi.zip --wait\n");
});
