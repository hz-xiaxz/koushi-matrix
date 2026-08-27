// @vitest-environment jsdom

import { afterEach, describe, expect, test, vi } from "vitest";

import type { DesktopApi } from "../backend/desktopApi";
import { createViewportSyncReporter } from "./viewportSyncReporter";

function rect(width = 1200, height = 800): DOMRect {
  return {
    bottom: height,
    height,
    left: 0,
    right: width,
    top: 0,
    width,
    x: 0,
    y: 0,
    toJSON: () => ({})
  } as DOMRect;
}

function installGeometry() {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200 });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
  Object.defineProperty(document.documentElement, "clientWidth", {
    configurable: true,
    value: 1200
  });
  Object.defineProperty(document.documentElement, "clientHeight", {
    configurable: true,
    value: 800
  });
  Object.defineProperty(window, "visualViewport", {
    configurable: true,
    value: { width: 1200, height: 800, offsetLeft: 0, offsetTop: 0 }
  });

  document.body.innerHTML = '<div id="root"><div class="desktop"></div></div>';
  for (const element of [document.body, document.querySelector("#root"), document.querySelector(".desktop")]) {
    Object.defineProperty(element, "getBoundingClientRect", {
      configurable: true,
      value: () => rect()
    });
  }
}

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("viewport sync reporter", () => {
  test("takes one finite post-commit observation for each explicit trigger", async () => {
    installGeometry();
    const observeViewportSync = vi.fn().mockResolvedValue({
      generation: 1,
      nativeSupport: "unsupported",
      decision: "unsupported",
      nativeAligned: false,
      domAligned: true,
      domJsAligned: true,
      domRootAligned: true
    });
    const api = { observeViewportSync } as unknown as DesktopApi;
    const report = createViewportSyncReporter(api);

    for (const density of ["default", "compact", "default", "comfortable"] as const) {
      await report("density_commit", density);
    }
    await report("browser_resize", "comfortable");

    expect(observeViewportSync).toHaveBeenCalledTimes(5);
    for (const [observation] of observeViewportSync.mock.calls) {
      expect(observation.trigger).toMatch(/^(density_commit|browser_resize)$/);
      expect(observation.density).toBeTruthy();
      expect(observation.window.width).toBe(1200);
      expect(observation.window.height).toBe(800);
      expect(observation.root.width).toBe(1200);
      expect(observation.root.height).toBe(800);
      expect(Object.values(observation.window).every(Number.isFinite)).toBe(true);
      expect(Object.values(observation.root).every(Number.isFinite)).toBe(true);
    }
    for (const density of ["default", "compact", "comfortable"] as const) {
      expect(
        observeViewportSync.mock.calls.filter(
          ([observation]) =>
            observation.trigger === "density_commit" && observation.density === density
        )
      ).toHaveLength(density === "default" ? 2 : 1);
    }
  });

  test("does not submit an incomplete DOM measurement", async () => {
    installGeometry();
    Object.defineProperty(window, "innerWidth", { configurable: true, value: Number.NaN });
    const observeViewportSync = vi.fn();
    const report = createViewportSyncReporter({ observeViewportSync } as unknown as DesktopApi);

    await report("density_commit", "default");

    expect(observeViewportSync).not.toHaveBeenCalled();
  });
});
