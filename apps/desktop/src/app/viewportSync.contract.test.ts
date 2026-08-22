import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

const reporterSource = readFileSync(new URL("./viewportSyncReporter.ts", import.meta.url), "utf8");
const appSource = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
const rustSource = readFileSync(
  new URL("../../src-tauri/src/viewport_sync.rs", import.meta.url),
  "utf8"
);
const diagnosticsSource = readFileSync(
  new URL("../../src-tauri/src/commands/diagnostics.rs", import.meta.url),
  "utf8"
);
const tauriLibSource = readFileSync(new URL("../../src-tauri/src/lib.rs", import.meta.url), "utf8");
const productionRustSource = rustSource.split("#[cfg(test)]")[0];
const productionDiagnosticsSource = diagnosticsSource.split("#[cfg(any(debug_assertions, test))]")[0];

describe("viewport synchronization source boundary", () => {
  test("frontend observation is one-shot and never owns native geometry", () => {
    expect(reporterSource).not.toMatch(/setTimeout|setInterval|requestAnimationFrame|ResizeObserver/);
    expect(reporterSource).not.toMatch(/expected(?:Viewport|Width|Height)|window\.resizeTo|dispatchEvent/);
    expect(appSource).not.toMatch(/expected(?:Viewport|Width|Height)|window\.resizeTo/);
    expect(appSource).toContain("createViewportSyncReporter");
  });

  test("native access and repair stay isolated in the adapter", () => {
    expect(productionRustSource).toContain("run_on_main_thread");
    expect(productionRustSource).toContain("NSView");
    expect(productionRustSource).toContain("setFrame");
    expect(productionRustSource).not.toMatch(/NSWindow|set_size|set_inner_size|setContentSize|layoutIfNeeded/);
    expect(productionRustSource).not.toMatch(/tokio::time|sleep\(|debounce|retry/);
  });

  test("native-only receipts never publish an incomplete DOM QA result", () => {
    const scheduleStart = tauriLibSource.indexOf("fn schedule_native_viewport_sync");
    const scheduleEnd = tauriLibSource.indexOf("#[cfg(target_os = \"macos\")]", scheduleStart);
    const scheduleSource = tauriLibSource.slice(scheduleStart, scheduleEnd);
    expect(scheduleSource).toContain("synchronize_and_record");
    expect(scheduleSource).not.toContain("update_qa_window_title_from_viewport_receipt");
  });

  test("diagnostic source is typed and does not accept raw error or private-data fields", () => {
    expect(productionDiagnosticsSource).toContain("observe_viewport_sync");
    expect(productionDiagnosticsSource).not.toMatch(
      /raw_error|room_id|user_id|event_id|url|selector|screenshot/
    );
  });
});
