import { describe, expect, test, vi } from "vitest";

import {
  focusedTimelineKey,
  roomTimelineKey,
  threadTimelineKey
} from "../domain/coreEvents";
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
const THREAD_KEY = threadTimelineKey(
  "@alice:example.invalid",
  "!a:example.invalid",
  "$root:example.invalid"
);
const FOCUSED_KEY = focusedTimelineKey(
  "@alice:example.invalid",
  "!a:example.invalid",
  "$target:example.invalid"
);
const REQUEST = { connection_id: 4, sequence: 8 };

function projection(
  delivery: ReturnType<typeof createTimelineAcknowledgementDelivery>,
  key = KEY_A,
  actorGeneration = 9,
  generation = 1
) {
  return delivery.acknowledgeProjection(
    REQUEST,
    key,
    actorGeneration,
    generation,
    3,
    true
  );
}

async function flushRejection(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("timeline acknowledgement delivery", () => {
  test("retries a pre-Core rejection and settles on queue acceptance", async () => {
    const clock = manualScheduler();
    const submitProjection = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("queue unavailable"))
      .mockResolvedValue(undefined);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: clock.scheduler
    });

    const accepted = projection(delivery);
    await flushRejection();
    expect(submitProjection).toHaveBeenCalledTimes(1);
    expect(clock.pendingDelays()).toEqual([50]);

    clock.runNext();
    await accepted;
    expect(submitProjection).toHaveBeenCalledTimes(2);
    expect(clock.pendingDelays()).toEqual([]);
  });

  test("coalesces duplicate delivery and remembers an accepted identity", async () => {
    const pending = deferred();
    const submitProjection = vi.fn(() => pending.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: manualScheduler().scheduler
    });

    const first = projection(delivery);
    const duplicate = projection(delivery);
    expect(duplicate).toBe(first);
    expect(submitProjection).toHaveBeenCalledOnce();

    pending.resolve();
    await first;
    await projection(delivery);
    expect(submitProjection).toHaveBeenCalledOnce();
  });

  test("an accepted identity does not suppress a replay from a new actor", async () => {
    const submitProjection = vi.fn(async () => undefined);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: manualScheduler().scheduler
    });

    await projection(delivery, KEY_A, 9, 1);
    await projection(delivery, KEY_A, 10, 1);

    expect(submitProjection).toHaveBeenCalledTimes(2);
  });

  test("fences A-B-A replacement and ignores the old late completion", async () => {
    const attempts = [deferred(), deferred(), deferred()];
    const submitProjection = vi.fn(() => attempts[submitProjection.mock.calls.length - 1]!.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: manualScheduler().scheduler
    });

    const oldA = projection(delivery, KEY_A, 9, 1);
    const oldARejected = expect(oldA).rejects.toThrow("superseded");
    const currentB = projection(delivery, KEY_B, 10, 2);
    await oldARejected;
    attempts[0]!.resolve();
    await flushRejection();
    expect(submitProjection).toHaveBeenCalledTimes(2);

    attempts[1]!.resolve();
    await currentB;
    const newA = projection(delivery, KEY_A, 11, 3);
    attempts[2]!.resolve();
    await newA;
    expect(submitProjection).toHaveBeenCalledTimes(3);
  });

  test("returning to an accepted identity still supersedes newer pending work", async () => {
    const pendingB = deferred();
    const submitProjection = vi
      .fn<() => Promise<void>>()
      .mockResolvedValueOnce(undefined)
      .mockReturnValueOnce(pendingB.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: manualScheduler().scheduler
    });

    await projection(delivery, KEY_A, 9, 1);
    const pending = projection(delivery, KEY_B, 10, 2);
    const pendingFailure = expect(pending).rejects.toThrow("superseded");
    await projection(delivery, KEY_A, 9, 1);
    await pendingFailure;
    pendingB.resolve();
    await flushRejection();

    expect(submitProjection).toHaveBeenCalledTimes(2);
  });

  test("stops after seven attempts with no remaining retry", async () => {
    const clock = manualScheduler();
    const submitProjection = vi.fn<() => Promise<void>>().mockRejectedValue(new Error("down"));
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: clock.scheduler
    });

    const failed = projection(delivery);
    const failure = expect(failed).rejects.toThrow("exhausted");
    for (const delay of [50, 100, 200, 400, 800, 1600]) {
      await flushRejection();
      expect(clock.pendingDelays()).toEqual([delay]);
      clock.runNext();
    }
    await failure;
    await expect(projection(delivery)).rejects.toThrow("exhausted");
    expect(submitProjection).toHaveBeenCalledTimes(7);
    expect(clock.pendingDelays()).toEqual([]);
  });

  test("keeps Room, Thread, Focused and repair jobs independent", async () => {
    const attempts = [deferred(), deferred(), deferred()];
    const repairAttempt = deferred();
    const submitProjection = vi.fn(
      () => attempts[submitProjection.mock.calls.length - 1]!.promise
    );
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(() => repairAttempt.promise),
      scheduler: manualScheduler().scheduler
    });

    const roomPromise = projection(delivery, KEY_A, 9, 1);
    const threadPromise = projection(delivery, THREAD_KEY, 9, 1);
    const focusedPromise = projection(delivery, FOCUSED_KEY, 9, 1);
    const repairPromise = delivery.acknowledgeRenderedBatch(KEY_A, 9, 3, 11, 5);
    expect(submitProjection).toHaveBeenCalledTimes(3);
    for (const attempt of attempts) attempt.resolve();
    repairAttempt.resolve();
    await Promise.all([roomPromise, threadPromise, focusedPromise, repairPromise]);
  });

  test("reset and dispose cancel timers and fence late completions", async () => {
    const clock = manualScheduler();
    const firstAttempt = deferred();
    const submitProjection = vi.fn(() => firstAttempt.promise);
    const delivery = createTimelineAcknowledgementDelivery({
      submitProjection,
      submitRepair: vi.fn(async () => undefined),
      scheduler: clock.scheduler
    });

    const pending = projection(delivery);
    const resetFailure = expect(pending).rejects.toThrow("reset");
    delivery.reset();
    await resetFailure;
    firstAttempt.resolve();
    await flushRejection();

    submitProjection.mockRejectedValueOnce(new Error("down"));
    const afterReset = projection(delivery, KEY_A, 10, 2);
    const disposedFailure = expect(afterReset).rejects.toThrow("disposed");
    await flushRejection();
    expect(clock.pendingDelays()).toEqual([50]);
    delivery.dispose();
    await disposedFailure;
    expect(clock.pendingDelays()).toEqual([]);
    await expect(projection(delivery, KEY_A, 11, 3)).rejects.toThrow("disposed");
  });
});
