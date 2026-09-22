// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNowMs } from "./useNowMs";

describe("useNowMs", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-21T12:00:00Z"));
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("advances on its own interval so relative labels do not need an outside render", () => {
    const { result } = renderHook(() => useNowMs(30_000));
    const start = result.current;

    act(() => vi.advanceTimersByTime(29_000));
    expect(result.current).toBe(start);

    act(() => vi.advanceTimersByTime(1_000));
    expect(result.current).toBe(start + 30_000);
  });

  it("stops its timer on unmount", () => {
    const { unmount } = renderHook(() => useNowMs(30_000));
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
