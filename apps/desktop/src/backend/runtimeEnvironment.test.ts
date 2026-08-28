/* @vitest-environment jsdom */

import { afterEach, describe, expect, test } from "vitest";

import { isTauriRuntime } from "./runtimeEnvironment";

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("runtime environment", () => {
  test("detects only the Tauri internals marker", () => {
    expect(isTauriRuntime()).toBe(false);
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {}
    });
    expect(isTauriRuntime()).toBe(true);
  });
});
