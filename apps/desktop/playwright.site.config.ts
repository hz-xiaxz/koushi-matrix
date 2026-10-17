import { defineConfig } from "@playwright/test";

// Website screenshot lane (site/README.md). Same determinism settings as the
// README lane, on its own port so both can run in one container session.
const SITE_SCREENSHOT_PORT = 5185;

export default defineConfig({
  testDir: "./e2e-docs/site",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${SITE_SCREENSHOT_PORT}`,
    headless: true,
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    reducedMotion: "reduce"
  },
  webServer: {
    command: `npx vite --port ${SITE_SCREENSHOT_PORT}`,
    url: `http://127.0.0.1:${SITE_SCREENSHOT_PORT}/appHarness.html`,
    reuseExistingServer: false,
    timeout: 30_000
  }
});
