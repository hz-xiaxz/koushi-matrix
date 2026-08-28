import type { RequestId, TimelineKey } from "../domain/coreEvents";
import { timelineStoreKeyId } from "../domain/timelineStore";

const MAX_DELIVERY_ATTEMPTS = 7;
const RETRY_DELAYS_MS = [50, 100, 200, 400, 800, 1600] as const;

export interface TimelineAcknowledgementScheduler {
  schedule(delayMs: number, callback: () => void): number;
  cancel(handle: number): void;
}

type ProjectionSubmission = (
  projectionRequestId: RequestId,
  key: TimelineKey,
  generation: number,
  itemCount: number,
  targetPresent: boolean
) => Promise<void>;

type RepairSubmission = (
  key: TimelineKey,
  actorGeneration: number,
  timelineGeneration: number,
  repairGeneration: number,
  batchId: number
) => Promise<void>;

type DeliveryJob = {
  identity: string;
  attempts: number;
  timer: number | null;
  settled: boolean;
  send: () => Promise<void>;
  promise: Promise<void>;
  resolve: () => void;
  reject: (error: Error) => void;
};

type DeliveryChannel = {
  active: DeliveryJob | null;
  acceptedIdentity: string | null;
  exhaustedIdentity: string | null;
};

export interface TimelineAcknowledgementDelivery {
  acknowledgeProjection(
    projectionRequestId: RequestId,
    key: TimelineKey,
    actorGeneration: number,
    generation: number,
    itemCount: number,
    targetPresent: boolean
  ): Promise<void>;
  acknowledgeRenderedBatch(
    key: TimelineKey,
    actorGeneration: number,
    timelineGeneration: number,
    repairGeneration: number,
    batchId: number
  ): Promise<void>;
  reset(): void;
  dispose(): void;
}

export function createTimelineAcknowledgementDelivery({
  submitProjection,
  submitRepair,
  scheduler = browserScheduler
}: {
  submitProjection: ProjectionSubmission;
  submitRepair: RepairSubmission;
  scheduler?: TimelineAcknowledgementScheduler;
}): TimelineAcknowledgementDelivery {
  const channel = (): DeliveryChannel => ({
    active: null,
    acceptedIdentity: null,
    exhaustedIdentity: null
  });
  const roomProjection = channel();
  const threadProjection = channel();
  const focusedProjection = channel();
  const repair = channel();
  const channels = [roomProjection, threadProjection, focusedProjection, repair];
  let disposed = false;

  function projectionChannel(key: TimelineKey): DeliveryChannel {
    if ("Room" in key.kind) return roomProjection;
    if ("Thread" in key.kind) return threadProjection;
    return focusedProjection;
  }

  function cancelJob(channel: DeliveryChannel, reason: "superseded" | "reset" | "disposed") {
    const job = channel.active;
    if (!job) return;
    channel.active = null;
    job.settled = true;
    if (job.timer !== null) {
      scheduler.cancel(job.timer);
      job.timer = null;
    }
    job.reject(new Error(`timeline acknowledgement ${reason}`));
  }

  function attempt(channel: DeliveryChannel, job: DeliveryJob): void {
    if (disposed || channel.active !== job || job.settled) return;
    job.attempts += 1;
    let submission: Promise<void>;
    try {
      submission = job.send();
    } catch {
      submission = Promise.reject(new Error("timeline acknowledgement submit failed"));
    }
    void submission.then(
      () => {
        if (channel.active !== job || job.settled) return;
        channel.active = null;
        channel.acceptedIdentity = job.identity;
        channel.exhaustedIdentity = null;
        job.settled = true;
        job.resolve();
      },
      () => {
        if (channel.active !== job || job.settled) return;
        if (job.attempts >= MAX_DELIVERY_ATTEMPTS) {
          channel.active = null;
          channel.exhaustedIdentity = job.identity;
          job.settled = true;
          job.reject(new Error("timeline acknowledgement delivery exhausted"));
          return;
        }
        const delayMs = RETRY_DELAYS_MS[job.attempts - 1]!;
        job.timer = scheduler.schedule(delayMs, () => {
          if (channel.active !== job || job.settled) return;
          job.timer = null;
          attempt(channel, job);
        });
      }
    );
  }

  function deliver(
    channel: DeliveryChannel,
    identity: string,
    send: () => Promise<void>
  ): Promise<void> {
    if (disposed) {
      return Promise.reject(new Error("timeline acknowledgement disposed"));
    }
    if (channel.active?.identity === identity) {
      return channel.active.promise;
    }
    if (channel.active) {
      cancelJob(channel, "superseded");
    }
    if (channel.acceptedIdentity === identity) {
      return Promise.resolve();
    }
    if (channel.exhaustedIdentity === identity) {
      return Promise.reject(new Error("timeline acknowledgement delivery exhausted"));
    }
    channel.exhaustedIdentity = null;
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<void>((nextResolve, nextReject) => {
      resolve = nextResolve;
      reject = nextReject;
    });
    const job: DeliveryJob = {
      identity,
      attempts: 0,
      timer: null,
      settled: false,
      send,
      promise,
      resolve,
      reject
    };
    channel.active = job;
    attempt(channel, job);
    return promise;
  }

  function reset(): void {
    for (const channel of channels) {
      cancelJob(channel, "reset");
      channel.acceptedIdentity = null;
      channel.exhaustedIdentity = null;
    }
  }

  return {
    acknowledgeProjection(
      projectionRequestId,
      key,
      actorGeneration,
      generation,
      itemCount,
      targetPresent
    ) {
      const identity = [
        timelineStoreKeyId(key),
        actorGeneration,
        projectionRequestId.connection_id,
        projectionRequestId.sequence,
        generation,
        itemCount,
        targetPresent ? 1 : 0
      ].join("\u0000");
      return deliver(projectionChannel(key), identity, () =>
        submitProjection(projectionRequestId, key, generation, itemCount, targetPresent)
      );
    },
    acknowledgeRenderedBatch(
      key,
      actorGeneration,
      timelineGeneration,
      repairGeneration,
      batchId
    ) {
      const identity = [
        timelineStoreKeyId(key),
        actorGeneration,
        timelineGeneration,
        repairGeneration,
        batchId
      ].join("\u0000");
      return deliver(repair, identity, () =>
        submitRepair(key, actorGeneration, timelineGeneration, repairGeneration, batchId)
      );
    },
    reset,
    dispose() {
      if (disposed) return;
      for (const channel of channels) {
        cancelJob(channel, "disposed");
        channel.acceptedIdentity = null;
        channel.exhaustedIdentity = null;
      }
      disposed = true;
    }
  };
}

const browserScheduler: TimelineAcknowledgementScheduler = {
  schedule(delayMs, callback) {
    return window.setTimeout(callback, delayMs);
  },
  cancel(handle) {
    window.clearTimeout(handle);
  }
};
