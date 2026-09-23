import { expect, type Page } from "@playwright/test";

import type { StateUpdateEnvelope } from "../../src/domain/coreEvents";
import type { DesktopSnapshot } from "../../src/domain/types";

interface StateUpdateHarness {
  currentSnapshot(): DesktopSnapshot;
  pushStateUpdate(update: StateUpdateEnvelope): void;
  appStoreGeneration(): number | null;
}

type ChangedSlices = Extract<StateUpdateEnvelope, { kind: "delta" }>["changed"];

/**
 * Push one delta at the generation after the one the harness is on, and wait
 * until the App's store has reached it — failing if the update was dropped.
 *
 * The generation is read inside the same `page.evaluate` that pushes (#984).
 * A generation derived from a snapshot read earlier goes stale whenever one of
 * the harness's own startup publishes lands in between, and the store then
 * drops the delta silently as already subsumed (#759) — which is correct
 * transport behaviour, so it is the spec that has to stay current.
 */
export async function pushDelta(page: Page, changed: ChangedSlices): Promise<number> {
  const generation = await page.evaluate((nextChanged) => {
    const harness = (window as unknown as { __harness: StateUpdateHarness }).__harness;
    const next = (harness.currentSnapshot().state_generation ?? 0) + 1;
    harness.pushStateUpdate({
      protocol_version: 1,
      kind: "delta",
      generation: next,
      changed: nextChanged
    } as StateUpdateEnvelope);
    return next;
  }, changed);
  await expectGeneration(page, generation);
  return generation;
}

/**
 * Push a full snapshot at the generation after the one the harness is on. The
 * caller's snapshot supplies the content; only its generation is replaced, for
 * the same reason as {@link pushDelta}.
 */
export async function pushSnapshot(page: Page, snapshot: DesktopSnapshot): Promise<number> {
  const generation = await page.evaluate((nextSnapshot) => {
    const harness = (window as unknown as { __harness: StateUpdateHarness }).__harness;
    const next = (harness.currentSnapshot().state_generation ?? 0) + 1;
    harness.pushStateUpdate({
      protocol_version: 1,
      kind: "snapshot",
      generation: next,
      reason: "settlement",
      snapshot: { ...nextSnapshot, state_generation: next }
    } as StateUpdateEnvelope);
    return next;
  }, snapshot);
  await expectGeneration(page, generation);
  return generation;
}

/**
 * The pushed generation is one past the harness's, and the store never runs
 * ahead of the harness, so the store reaching it proves the update landed. A
 * dropped update leaves the store behind and times this out.
 */
async function expectGeneration(page: Page, generation: number): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as unknown as { __harness: StateUpdateHarness }).__harness.appStoreGeneration() ??
          0
      )
    )
    .toBeGreaterThanOrEqual(generation);
}
