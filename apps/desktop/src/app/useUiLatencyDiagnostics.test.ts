// @vitest-environment jsdom
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUiLatencyDiagnostics } from "./useUiLatencyDiagnostics";

describe("useUiLatencyDiagnostics", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("cancels its owned animation frame on unmount", () => {
    const frameId = 551;
    const requestAnimationFrame = vi
      .spyOn(window, "requestAnimationFrame")
      .mockReturnValue(frameId);
    const cancelAnimationFrame = vi.spyOn(window, "cancelAnimationFrame");

    const { unmount } = renderHook(() => useUiLatencyDiagnostics());

    expect(requestAnimationFrame).toHaveBeenCalledWith(expect.any(Function));
    unmount();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(frameId);
  });
});
