import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { expect, test } from "vitest";

import { repoRoot } from "./releaseTestSupport";

function printedBuildConfig(args: string[] = [], signingEnvironment: Record<string, string> = {}) {
  const directory = mkdtempSync(join(tmpdir(), "koushi-dmg-signing-"));
  try {
    // Inventory only synthetic identities; never access the user's keychain.
    writeFileSync(join(directory, "security"), `#!/bin/sh
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
  echo '1) AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA "Developer ID Application: Synthetic"'
else
  echo '0 valid identities found'
fi
`, {
      mode: 0o700
    });
    const result = spawnSync(process.execPath, [
      "scripts/desktop-build-dmg.mjs", "--print-command", ...args
    ], {
      cwd: repoRoot,
      encoding: "utf8",
      timeout: 15_000,
      env: {
        PATH: `${directory}${delimiter}${process.env.PATH ?? ""}`,
        ...signingEnvironment
      }
    });
    expect(result.status, result.stderr).toBe(0);
    return JSON.parse(result.stdout.match(/--config (.+)/)?.[1] ?? "null");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

test("a local DMG without a certificate explicitly signs the whole app ad hoc", () => {
  expect(printedBuildConfig().bundle.macOS.signingIdentity).toBe("-");
});

test("a signed release never falls back to ad-hoc signing", () => {
  expect(printedBuildConfig(["--signed"]).bundle.macOS.signingIdentity).toBeUndefined();
});

test("a certificate supplied for import is not replaced by ad-hoc signing", () => {
  expect(printedBuildConfig([], {
    APPLE_CERTIFICATE: "synthetic-certificate",
    APPLE_CERTIFICATE_PASSWORD: "synthetic-password"
  }).bundle.macOS.signingIdentity).toBeUndefined();
});

test("an explicitly selected valid identity is preserved in the build environment", () => {
  expect(printedBuildConfig([], {
    APPLE_SIGNING_IDENTITY: "Developer ID Application: Synthetic"
  }).bundle.macOS.signingIdentity).toBeUndefined();
});
