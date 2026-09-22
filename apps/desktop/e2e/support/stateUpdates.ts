import { expect, type Page } from "@playwright/test";

import type { StateUpdateEnvelope } from "../../src/domain/coreEvents";
import type { DesktopSnapshot } from "../../src/domain/types";

interface StateUpdateHarness {
  currentSnapshot(): DesktopSnapshot;
  pushStateUpdate(update: StateUpdateEnvelope): void;
  appStoreGeneration(): number | null;
  appStoreDeltaStats(): { applied: number; staleIgnored: number; gapRefreshRequested: number };
}

type ChangedSlices = Extract<StateUpdateEnvelope, { kind: "delta" }>["changed"];

/**
 * Push one delta at the generation after the one the harness is on, and wait
 * until the App's store has applied it.
 *
 * The generation is read inside the same `page.evaluate` that pushes (#984).
 * A generation derived from a snapshot read earlier goes stale whenever one of
 * the harness's own startup publishes lands in between, and the store then
 * drops the delta silently as already subsumed (#759) — which is correct
 * transport behaviour, so it is the spec that has to stay current.
 */
export async function pushDelta(page: Page, changed: ChangedSlices): Promise<number> {
  const { generation, staleIgnored } = await page.evaluate((nextChanged) => {
    const harness = (window as unknown as { __harness: StateUpdateHarness }).__harness;
    const next = (harness.currentSnapshot().state_generation ?? 0) + 1;
    const before = harness.appStoreDeltaStats().staleIgnored;
    harness.pushStateUpdate({
      protocol_version: 1,
      kind: "delta",
      generation: next,
      changed: nextChanged
    } as StateUpdateEnvelope);
    return { generation: next, staleIgnored: before };
  }, changed);
  await expectApplied(page, generation, staleIgnored);
  return generation;
}

/**
 * Push a full snapshot at the generation after the one the harness is on. The
 * caller's snapshot supplies the content; only its generation is replaced, for
 * the same reason as {@link pushDelta}.
 */
export async function pushSnapshot(page: Page, snapshot: DesktopSnapshot): Promise<number> {
  const { generation, staleIgnored } = await page.evaluate((nextSnapshot) => {
    const harness = (window as unknown as { __harness: StateUpdateHarness }).__harness;
    const next = (harness.currentSnapshot().state_generation ?? 0) + 1;
    const before = harness.appStoreDeltaStats().staleIgnored;
    harness.pushStateUpdate({
      protocol_version: 1,
      kind: "snapshot",
      generation: next,
      reason: "settlement",
      snapshot: { ...nextSnapshot, state_generation: next }
    } as StateUpdateEnvelope);
    return { generation: next, staleIgnored: before };
  }, snapshot);
  await expectApplied(page, generation, staleIgnored);
  return generation;
}

async function expectApplied(page: Page, generation: number, staleIgnored: number): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as unknown as { __harness: StateUpdateHarness }).__harness.appStoreGeneration() ??
          0
      )
    )
    .toBeGreaterThanOrEqual(generation);
  const stats = await page.evaluate(() =>
    (window as unknown as { __harness: StateUpdateHarness }).__harness.appStoreDeltaStats()
  );
  expect(stats.staleIgnored, "the pushed update was dropped as stale").toBe(staleIgnored);
}
