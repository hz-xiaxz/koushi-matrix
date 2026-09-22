// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUiLatencyDiagnostics } from "./useUiLatencyDiagnostics";

/** Drives the hook's animation-frame loop by hand, one timestamp per frame. */
function installFrameDriver() {
  let pending: FrameRequestCallback | null = null;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    pending = callback;
    return 1;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {
    pending = null;
  });
  return (now: number) => {
    const callback = pending;
    pending = null;
    act(() => callback?.(now));
  };
}

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

    const { unmount } = renderHook(() => useUiLatencyDiagnostics({ live: false }));

    expect(requestAnimationFrame).toHaveBeenCalledWith(expect.any(Function));
    unmount();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(frameId);
  });

  // Issue #969: the hook lives in `App`, so publishing a sample into React
  // state re-rendered the whole tree once per second for a value that is only
  // read when a diagnostic report is built.
  it("does not re-render its host while diagnostics are closed", () => {
    const frame = installFrameDriver();
    let renders = 0;
    const { result } = renderHook(() => {
      renders += 1;
      return useUiLatencyDiagnostics({ live: false });
    });
    const rendersAfterMount = renders;

    for (let now = 16; now <= 5000; now += 16) frame(now);

    expect(renders).toBe(rendersAfterMount);
    const read = result.current;
    expect(read().samples).toBeGreaterThan(300);
    expect(read().lastFrameGapMs).toBe(16);
  });

  it("keeps one reader identity across renders", () => {
    installFrameDriver();
    const { result, rerender } = renderHook(
      ({ live }: { live: boolean }) => useUiLatencyDiagnostics({ live }),
      { initialProps: { live: false } }
    );
    const first = result.current;
    rerender({ live: true });
    expect(result.current).toBe(first);
  });

  it("re-renders about once per second while diagnostics are open", () => {
    const frame = installFrameDriver();
    let renders = 0;
    const { rerender } = renderHook(
      ({ live }: { live: boolean }) => {
        renders += 1;
        return useUiLatencyDiagnostics({ live });
      },
      { initialProps: { live: false } }
    );
    for (let now = 16; now <= 2000; now += 16) frame(now);
    rerender({ live: true });
    const rendersWhenOpened = renders;

    for (let now = 2016; now <= 5000; now += 16) frame(now);

    expect(renders - rendersWhenOpened).toBeGreaterThanOrEqual(2);
    expect(renders - rendersWhenOpened).toBeLessThanOrEqual(4);
  });
});
