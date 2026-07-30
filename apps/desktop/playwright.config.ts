/**
 * Playwright config for the headless UI DOM tier (QA Model layer 4).
 *
 * Drives headless Chromium against the Vite-served harness page
 * (harness.html → src/test/harnessMain.tsx → real TimelineView + mock IPC).
 *
 * ABSOLUTELY NO GUI: headless only, no Tauri app, no native window. The Vite
 * dev server is started by Playwright on port 5183 (NOT the canonical 5173,
 * so a developer's running dev server is never clobbered) and is torn down
 * by Playwright when the run ends.
 */

import { defineConfig } from "@playwright/test";

const HARNESS_PORT = 5183;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  // One worker, not just one test per file. Playwright still spreads FILES
  // across workers when `fullyParallel` is false, and every flake recorded in
  // AGENTS.md was traced to those workers contending for the one shared Vite
  // harness server — a stale `get_snapshot` landing in another file's test.
  // Serializing removes that contention by construction and keeps CI and a
  // local run identical; the whole suite still finishes in a few minutes.
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${HARNESS_PORT}`,
    headless: true
  },
  webServer: {
    command: `npx vite --port ${HARNESS_PORT}`,
    url: `http://127.0.0.1:${HARNESS_PORT}/harness.html`,
    reuseExistingServer: false,
    timeout: 30_000,
    env: {
      // #370 disables device-to-device (SAS) verification in the shipped UI.
      // The gate specs here exercise that implementation — flow correlation,
      // mismatch, cancellation, retry — so the harness build keeps it enabled.
      //
      // This tier therefore does NOT prove the production default. That is
      // pinned by `SessionVerificationGate.test.tsx`, which asserts no SAS
      // button, no confirm dialog, no emoji comparison, and that
      // `startOwnUserSas` is never invoked when the flag is absent.
      VITE_KOUSHI_ENABLE_DEVICE_VERIFICATION: "1"
    }
  }
});
