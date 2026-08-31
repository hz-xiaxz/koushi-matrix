import { describe, expect, test, vi } from "vitest";

import { roomTimelineKey } from "../domain/coreEvents";
import { createTimelineAcknowledgementDelivery } from "./timelineAcknowledgementDelivery";

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

function manualScheduler() {
  let nextId = 0;
  const tasks = new Map<number, { delayMs: number; callback: () => void }>();
  return {
    scheduler: {
      schedule(delayMs: number, callback: () => void) {
        nextId += 1;
        tasks.set(nextId, { delayMs, callback });
        return nextId;
      },
      cancel(handle: number) {
        tasks.delete(handle);
      }
    },
    pendingDelays: () => [...tasks.values()].map((task) => task.delayMs),
    runNext() {
      const first = tasks.entries().next().value as
        | [number, { delayMs: number; callback: () => void }]
        | undefined;
      if (!first) throw new Error("no scheduled acknowledgement retry");
      tasks.delete(first[0]);
      first[1].callback();
    }
  };
}

const KEY_A = roomTimelineKey("@alice:example.invalid", "!a:example.invalid");
const KEY_B = roomTimelineKey("@alice:example.invalid", "!b:example.invalid");

function repair(
  delivery: ReturnType<typeof createTimelineAcknowledgementDelivery>,
  key = KEY_A,
  actorGeneration = 9,
  timelineGeneration = 3,
  repairGeneration = 11,
  batchId = 5
) {
  return delivery.acknowledgeRenderedBatch(
    key,
    actorGeneration,
    timelineGeneration,
    repairGeneration,
    batchId
  );
}

async function flushRejection(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("timeline repair acknowledgement delivery", () => {
  test("retries a pre-Core rejection and settles on queue acceptance", async () => {
    const clock = manualScheduler();
    const submitRepair = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("queue unavailable"))
      .mockResolvedValue(undefined);
    const delivery = createTimelineAcknowledgementDelivery({
      submitRepair,
      scheduler: clock.scheduler
    });

    const accepted = repair(delivery);
    await flushRejection();
    expect(submitRepair).toHaveBeenCalledTimes(1);
    expect(clock.pendingDelays()).toEqual([50]);

    clock.runNext();
    await accepted;
    expect(submitRepair).toHaveBeenCalledTimes(2);
    expect(clock.pendingDelays()).toEqual([]);
  });

  test("coalesces duplicate delivery and remembers an accepted identity", async () => {
    const pending = deferred();
    const submitRepair = vi.fn(() => pending.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitRepair,
      scheduler: manualScheduler().scheduler
    });

    const first = repair(delivery);
    const duplicate = repair(delivery);
    expect(duplicate).toBe(first);
    expect(submitRepair).toHaveBeenCalledOnce();

    pending.resolve();
    await first;
    await repair(delivery);
    expect(submitRepair).toHaveBeenCalledOnce();
  });

  test("a changed actor or key supersedes the previous identity", async () => {
    const attempts = [deferred(), deferred()];
    const submitRepair = vi.fn(
      () => attempts[submitRepair.mock.calls.length - 1]!.promise
    );
    const delivery = createTimelineAcknowledgementDelivery({
      submitRepair,
      scheduler: manualScheduler().scheduler
    });

    const old = repair(delivery, KEY_A, 9);
    const oldFailure = expect(old).rejects.toThrow("superseded");
    const current = repair(delivery, KEY_B, 10);
    await oldFailure;
    attempts[0]!.resolve();
    attempts[1]!.resolve();
    await current;
    expect(submitRepair).toHaveBeenCalledTimes(2);
  });

  test("stops after seven attempts with no remaining retry", async () => {
    const clock = manualScheduler();
    const submitRepair = vi.fn<() => Promise<void>>().mockRejectedValue(new Error("down"));
    const delivery = createTimelineAcknowledgementDelivery({
      submitRepair,
      scheduler: clock.scheduler
    });

    const failed = repair(delivery);
    const failure = expect(failed).rejects.toThrow("exhausted");
    for (const delay of [50, 100, 200, 400, 800, 1600]) {
      await flushRejection();
      expect(clock.pendingDelays()).toEqual([delay]);
      clock.runNext();
    }
    await failure;
    await expect(repair(delivery)).rejects.toThrow("exhausted");
    expect(submitRepair).toHaveBeenCalledTimes(7);
  });

  test("reset and dispose cancel pending work and fence late completions", async () => {
    const firstAttempt = deferred();
    const submitRepair = vi.fn(() => firstAttempt.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitRepair,
      scheduler: manualScheduler().scheduler
    });

    const pending = repair(delivery);
    const resetFailure = expect(pending).rejects.toThrow("reset");
    delivery.reset();
    await resetFailure;
    firstAttempt.resolve();
    await flushRejection();

    const afterReset = repair(delivery, KEY_A, 10);
    const disposedFailure = expect(afterReset).rejects.toThrow("disposed");
    delivery.dispose();
    await disposedFailure;
    await expect(repair(delivery, KEY_A, 11)).rejects.toThrow("disposed");
  });
});
