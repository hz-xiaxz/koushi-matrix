import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("manifest preserves both signed platform entries and rejects duplicate targets", () => {
  const directory = mkdtempSync(join(tmpdir(), "koushi-manifest-"));
  try {
    const signature = join(directory, "artifact.sig");
    const output = join(directory, "latest.json");
    writeFileSync(signature, "synthetic-signature");
    const args = ["scripts/desktop-updater-manifest.mjs", "--version", "1.2.3",
      "--target", "darwin-aarch64", "--url", "https://example.invalid/mac",
      "--signature-file", signature, "--target", "windows-x86_64",
      "--url", "https://example.invalid/windows.exe", "--signature-file", signature,
      "--output", output];
    const result = spawnSync(process.execPath, args, { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const manifest = JSON.parse(readFileSync(output, "utf8"));
    assert.deepEqual(Object.keys(manifest.platforms), ["darwin-aarch64", "windows-x86_64"]);
    assert.equal(manifest.platforms["windows-x86_64"].signature, "synthetic-signature");
    args[args.indexOf("windows-x86_64")] = "darwin-aarch64";
    assert.equal(spawnSync(process.execPath, args).status, 1);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
