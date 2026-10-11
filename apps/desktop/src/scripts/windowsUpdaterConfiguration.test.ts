import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { repoRoot } from "./releaseTestSupport";

test("Windows updater build requires trust material and emits no private key", () => {
  const environment = { ...process.env };
  delete environment.KOUSHI_UPDATER_PUBLIC_KEY;
  delete environment.TAURI_SIGNING_PRIVATE_KEY;
  delete environment.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
  const args = ["scripts/desktop-build-windows.mjs", "--print-command", "--updater"];
  const missing = spawnSync(process.execPath, args, { cwd: repoRoot, encoding: "utf8", env: environment });
  expect(missing.status).toBe(1);
  const configured = spawnSync(process.execPath, args, {
    cwd: repoRoot, encoding: "utf8", env: {
      ...environment, KOUSHI_UPDATER_PUBLIC_KEY: "synthetic-public-key",
      TAURI_SIGNING_PRIVATE_KEY: "synthetic-private-key",
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "synthetic-password"
    }
  });
  expect(configured.status).toBe(0);
  const config = JSON.parse(configured.stdout.match(/--config (.+)/)?.[1] ?? "null");
  expect(config.bundle.createUpdaterArtifacts).toBe(true);
  expect(config.plugins.updater.pubkey).toBe("synthetic-public-key");
  expect(configured.stdout + configured.stderr).not.toContain("synthetic-private-key");
  expect(configured.stdout + configured.stderr).not.toContain("synthetic-password");
});
