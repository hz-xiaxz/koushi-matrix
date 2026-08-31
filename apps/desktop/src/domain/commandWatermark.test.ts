import { describe, expect, test, vi } from "vitest";

import type { DesktopSnapshot } from "./types";
import { createCommandReceiptReconciler } from "./commandWatermark";

function snapshot(generation: number): DesktopSnapshot {
  return { state_generation: generation } as DesktopSnapshot;
}

describe("command receipt watermark", () => {
  test("does not resync when the delta arrived before the promise", async () => {
    const settlementSnapshot = vi.fn(async () => snapshot(4));
    const reconcile = createCommandReceiptReconciler({
      currentGeneration: () => 4,
      settlementSnapshot,
      applySnapshot: vi.fn()
    });
    await reconcile({ protocolVersion: 1, publishedGeneration: 4 });
    expect(settlementSnapshot).not.toHaveBeenCalled();
  });

  test("coalesces concurrent receipts into one state-only resync", async () => {
    let generation = 1;
    let resolveSnapshot!: (snapshot: DesktopSnapshot) => void;
    const settlementSnapshot = vi.fn(
      () => new Promise<DesktopSnapshot>((resolve) => { resolveSnapshot = resolve; })
    );
    const reconcile = createCommandReceiptReconciler({
      currentGeneration: () => generation,
      settlementSnapshot,
      applySnapshot: (next) => { generation = next.state_generation ?? generation; }
    });

    const first = reconcile({ protocolVersion: 1, publishedGeneration: 2 });
    const second = reconcile({ protocolVersion: 1, admittedGeneration: 3 });
    resolveSnapshot(snapshot(3));
    await Promise.all([first, second]);

    expect(settlementSnapshot).toHaveBeenCalledOnce();
    expect(generation).toBe(3);
  });

  test("performs one monotone state-only resync when the promise arrives first", async () => {
    let generation = 3;
    const applySnapshot = vi.fn((next: DesktopSnapshot) => {
      generation = next.state_generation ?? generation;
    });
    const settlementSnapshot = vi.fn(async () => snapshot(4));

    const reconcile = createCommandReceiptReconciler({
      currentGeneration: () => generation,
      settlementSnapshot,
      applySnapshot
    });
    await reconcile({ protocolVersion: 1, admittedGeneration: 4 });

    expect(settlementSnapshot).toHaveBeenCalledOnce();
    expect(applySnapshot).toHaveBeenCalledWith(snapshot(4));
    expect(generation).toBe(4);
  });

  test("does not regress when the ordered lane advances during settlement read", async () => {
    let generation = 1;
    let resolveSnapshot!: (snapshot: DesktopSnapshot) => void;
    const applySnapshot = vi.fn();
    const reconcile = createCommandReceiptReconciler({
      currentGeneration: () => generation,
      settlementSnapshot: () =>
        new Promise<DesktopSnapshot>((resolve) => {
          resolveSnapshot = resolve;
        }),
      applySnapshot
    });

    const pending = reconcile({ protocolVersion: 1, publishedGeneration: 2 });
    generation = 3;
    resolveSnapshot(snapshot(2));
    await pending;

    expect(applySnapshot).not.toHaveBeenCalled();
    expect(generation).toBe(3);
  });
});
