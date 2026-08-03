import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { writeValidatedQaOutputFiles } from "./qa-output-artifacts.mjs";

function artifactPaths(directory, label) {
  return {
    stdout: join(directory, `${label}-stdout.log`),
    stderr: join(directory, `${label}-stderr.log`)
  };
}

function rejectUnsafeOutput({ stdout, stderr }) {
  if (stdout.includes("unsafe") || stderr.includes("unsafe")) {
    throw new Error("unsafe output rejected");
  }
}

for (const [label, output] of [
  ["sdk", { stdout: "unsafe stdout", stderr: "safe stderr" }],
  ["core-sync-service", { stdout: "safe stdout", stderr: "unsafe stderr" }]
]) {
  test(`${label} validation failure leaves no uploadable output artifacts`, () => {
    const directory = mkdtempSync(join(tmpdir(), "koushi-qa-artifacts-test-"));
    const paths = artifactPaths(directory, label);
    try {
      assert.throws(
        () =>
          writeValidatedQaOutputFiles({
            directory,
            label,
            ...output,
            validate: rejectUnsafeOutput
          }),
        /unsafe output rejected/
      );
      assert.equal(existsSync(paths.stdout), false);
      assert.equal(existsSync(paths.stderr), false);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
}

test("validated output is written to separate stdout and stderr artifacts", () => {
  const directory = mkdtempSync(join(tmpdir(), "koushi-qa-artifacts-test-"));
  const label = "sdk";
  const paths = artifactPaths(directory, label);
  try {
    writeValidatedQaOutputFiles({
      directory,
      label,
      stdout: "safe stdout",
      stderr: "safe stderr",
      validate: rejectUnsafeOutput
    });

    assert.equal(readFileSync(paths.stdout, "utf8"), "safe stdout");
    assert.equal(readFileSync(paths.stderr, "utf8"), "safe stderr");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
