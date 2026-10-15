/**
 * Playwright config for the headless UI DOM tier (QA Model layer 4).
 *
 * Drives headless Chromium against the Vite-served harness page
 * (harness.html → src/test/harnessMain.tsx → real TimelineView + mock IPC).
 * A short allow-list of engine-sensitive specs also runs in headless WebKit.
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
  // docs/agents/qa-lanes.md was traced to those workers contending for the one shared Vite
  // harness server — a stale `get_snapshot` landing in another file's test.
  // Serializing removes that contention by construction and keeps CI and a
  // local run identical; the whole suite still finishes in a few minutes.
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${HARNESS_PORT}`,
    headless: true,
    // Some headless environments (e.g. machines without a GPU/driver) stall
    // Chromium's compositor unless software GL is forced. Opt-in only via
    // KOUSHI_PLAYWRIGHT_EXTRA_ARGS (Koushi-prefixed per engineering-rules) so
    // the default run stays byte-identical:
    //   KOUSHI_PLAYWRIGHT_EXTRA_ARGS="--disable-gpu --enable-unsafe-swiftshader" \
    //     npx playwright test
    launchOptions: process.env.KOUSHI_PLAYWRIGHT_EXTRA_ARGS
      ? { args: process.env.KOUSHI_PLAYWRIGHT_EXTRA_ARGS.split(" ") }
      : undefined
  },
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    // WebKit covers the macOS WKWebView and Linux WebKitGTK engine family for
    // specs whose defect is engine-native (search-field decorations, #1145).
    // Opt a spec in by adding it here; everything else stays Chromium-only.
    {
      name: "webkit",
      // The engine-native wheel-scroll specs are the only recorded flake left
      // in this tier (#1425: the viewport moved by 0 px twice, then passed), and
      // they run here. Record a trace for this project only: a trace for all
      // 600 specs would slow the slowest gate and add the very I/O load the
      // flake correlates with. Failing tests keep their trace; passing ones
      // drop it. CI uploads `test-results/` on failure.
      use: { browserName: "webkit", trace: "retain-on-failure" },
      testMatch: [
        /room-list-filter-native-clear\.spec\.ts$/,
        /upload-preview-heic\.spec\.ts$/,
        /inline-math-baseline\.spec\.ts$/,
        /activity-shell-scroll\.spec\.ts$/,
        /timeline-scroll-anchor-drift\.spec\.ts$/,
        /scheduled-messages-layout\.spec\.ts$/,
        /scrollable-list-bounds\.spec\.ts$/,
        // Remote-text direction (#1378) is resolved by the engine's own bidi
        // implementation, and the mention pill's paint (#1328) is read from
        // computed styles, whose `color-mix()` serialization differs per engine.
        /thread-summary-bidi\.spec\.ts$/,
        /mention-pill-rendering\.spec\.ts$/,
        /thread-mention-pill\.spec\.ts$/,
        /upload-staging-scroll\.spec\.ts$/
      ]
    }
  ],
  webServer: {
    command: `npx vite --port ${HARNESS_PORT}`,
    url: `http://127.0.0.1:${HARNESS_PORT}/harness.html`,
    reuseExistingServer: false,
    timeout: 30_000
  }
});
