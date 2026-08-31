import { beforeEach, describe, expect, test, vi } from "vitest";

import { clearAppStoreSnapshot, getAppStoreSnapshot, setAppStoreSnapshot } from "./appStore";
import type { StateUpdateEnvelope } from "./coreEvents";
import { createStateUpdateConsumer } from "./stateUpdateConsumer";
import type { DesktopSnapshot } from "./types";

function snapshot(generation: number): DesktopSnapshot {
  return { state_generation: generation } as DesktopSnapshot;
}

describe("state update consumer", () => {
  beforeEach(() => clearAppStoreSnapshot());

  test("queues updates until the initial snapshot, applies contiguous deltas, and ignores stale duplicates", async () => {
    const applyDelta = vi.fn((envelope: Extract<StateUpdateEnvelope, { kind: "delta" }>) => {
      setAppStoreSnapshot({
        ...getAppStoreSnapshot()!,
        state_generation: envelope.generation
      });
      return true;
    });
    const resyncSnapshot = vi.fn(async () => snapshot(3));
    const consumer = createStateUpdateConsumer({
      applySnapshot: setAppStoreSnapshot,
      applyDelta,
      resetTimeline: vi.fn(),
      resyncSnapshot
    });

    consumer.receive({ protocol_version: 1, kind: "delta", generation: 2, changed: {} });
    consumer.initialize(snapshot(1));
    consumer.receive({ protocol_version: 1, kind: "delta", generation: 2, changed: {} });
    consumer.receive({ protocol_version: 1, kind: "delta", generation: 2, changed: {} });

    expect(applyDelta).toHaveBeenCalledOnce();
    expect(resyncSnapshot).not.toHaveBeenCalled();
    expect(getAppStoreSnapshot()?.state_generation).toBe(2);
  });

  test("resyncs one frontend generation gap, resets timeline once, and resumes monotonically", async () => {
    const resetTimeline = vi.fn();
    const resyncSnapshot = vi.fn(async () => snapshot(5));
    const consumer = createStateUpdateConsumer({
      applySnapshot: setAppStoreSnapshot,
      applyDelta: (envelope) => {
        const current = getAppStoreSnapshot()!;
        setAppStoreSnapshot({ ...current, state_generation: envelope.generation });
        return true;
      },
      resetTimeline,
      resyncSnapshot
    });
    consumer.initialize(snapshot(1));

    consumer.receive({ protocol_version: 1, kind: "delta", generation: 3, changed: {} });
    consumer.receive({ protocol_version: 1, kind: "delta", generation: 4, changed: {} });
    await vi.waitFor(() => expect(getAppStoreSnapshot()?.state_generation).toBe(5));
    consumer.receive({ protocol_version: 1, kind: "delta", generation: 6, changed: {} });

    expect(resetTimeline).toHaveBeenCalledOnce();
    expect(resyncSnapshot).toHaveBeenCalledOnce();
    expect(getAppStoreSnapshot()?.state_generation).toBe(6);
  });

  test("resyncs when appStore rejects a contiguous delta and resets on a lag snapshot", async () => {
    const resetTimeline = vi.fn();
    const resyncSnapshot = vi.fn(async () => snapshot(2));
    const consumer = createStateUpdateConsumer({
      applySnapshot: setAppStoreSnapshot,
      applyDelta: () => false,
      resetTimeline,
      resyncSnapshot
    });
    consumer.initialize(snapshot(1));

    consumer.receive({ protocol_version: 1, kind: "delta", generation: 2, changed: {} });
    await vi.waitFor(() => expect(resyncSnapshot).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(getAppStoreSnapshot()?.state_generation).toBe(2));

    consumer.receive({
      protocol_version: 1,
      kind: "snapshot",
      generation: 3,
      snapshot: snapshot(3),
      reason: "lag"
    });

    expect(resetTimeline).toHaveBeenCalledOnce();
    expect(getAppStoreSnapshot()?.state_generation).toBe(3);
  });

  test("falls back from an initial read failure and retries a failed resync only on a later update", async () => {
    const resetTimeline = vi.fn();
    const resyncSnapshot = vi
      .fn<() => Promise<DesktopSnapshot>>()
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockResolvedValueOnce(snapshot(4));
    const consumer = createStateUpdateConsumer({
      applySnapshot: setAppStoreSnapshot,
      applyDelta: () => true,
      resetTimeline,
      resyncSnapshot
    });

    consumer.receive({ protocol_version: 1, kind: "delta", generation: 3, changed: {} });
    consumer.recoverInitial();
    await vi.waitFor(() => expect(resyncSnapshot).toHaveBeenCalledOnce());
    expect(getAppStoreSnapshot()).toBeNull();

    consumer.receive({ protocol_version: 1, kind: "delta", generation: 4, changed: {} });
    await vi.waitFor(() => expect(resyncSnapshot).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(getAppStoreSnapshot()?.state_generation).toBe(4));
    expect(resetTimeline).toHaveBeenCalledTimes(2);
  });

  test("resyncs a mismatched snapshot envelope and ignores completion after dispose", async () => {
    let resolveResync!: (value: DesktopSnapshot) => void;
    const resyncSnapshot = vi.fn(
      () =>
        new Promise<DesktopSnapshot>((resolve) => {
          resolveResync = resolve;
        })
    );
    const applySnapshot = vi.fn(setAppStoreSnapshot);
    const consumer = createStateUpdateConsumer({
      applySnapshot,
      applyDelta: () => true,
      resetTimeline: vi.fn(),
      resyncSnapshot
    });
    consumer.initialize(snapshot(1));
    consumer.receive({
      protocol_version: 1,
      kind: "snapshot",
      generation: 2,
      snapshot: snapshot(3),
      reason: "lag"
    });
    expect(resyncSnapshot).toHaveBeenCalledOnce();

    consumer.dispose();
    resolveResync(snapshot(2));
    await Promise.resolve();

    expect(applySnapshot).toHaveBeenCalledOnce();
    expect(getAppStoreSnapshot()?.state_generation).toBe(1);
  });

  test("queues a lag snapshot during resync and applies it monotonically afterward", async () => {
    let resolveResync!: (value: DesktopSnapshot) => void;
    const consumer = createStateUpdateConsumer({
      applySnapshot: setAppStoreSnapshot,
      applyDelta: () => true,
      resetTimeline: vi.fn(),
      resyncSnapshot: () =>
        new Promise<DesktopSnapshot>((resolve) => {
          resolveResync = resolve;
        })
    });
    consumer.initialize(snapshot(1));
    consumer.receive({ protocol_version: 1, kind: "delta", generation: 3, changed: {} });
    consumer.receive({
      protocol_version: 1,
      kind: "snapshot",
      generation: 4,
      snapshot: snapshot(4),
      reason: "lag"
    });

    resolveResync(snapshot(3));
    await vi.waitFor(() => expect(getAppStoreSnapshot()?.state_generation).toBe(4));
  });
});
