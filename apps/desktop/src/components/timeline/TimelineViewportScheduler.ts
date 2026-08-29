import {
  scheduleTimelineFrame,
  type TimelineScheduledFrame
} from "./TimelineViewportVirtualization";

export type ViewportEpoch = number;

export interface TimelineViewportScheduler {
  currentEpoch(): ViewportEpoch;
  advance(): ViewportEpoch;
  schedule(epoch: ViewportEpoch, cb: FrameRequestCallback): TimelineScheduledFrame;
  cancelBefore(epoch: ViewportEpoch): void;
  dispose(): void;
}

type PendingFrame = {
  epoch: ViewportEpoch;
  frame: TimelineScheduledFrame;
  cancelled: boolean;
};

const cancelledFrame: TimelineScheduledFrame = { cancel: () => undefined };
const MAX_MANUAL_FLUSHES = 1_000;

export function createTimelineViewportScheduler(): TimelineViewportScheduler {
  let epoch = 0;
  let disposed = false;
  const pending = new Set<PendingFrame>();

  const cancel = (pendingFrame: PendingFrame) => {
    if (pendingFrame.cancelled) {
      return;
    }
    pendingFrame.cancelled = true;
    pending.delete(pendingFrame);
    pendingFrame.frame.cancel();
  };

  return {
    currentEpoch: () => epoch,
    advance: () => {
      if (!disposed) {
        epoch += 1;
      }
      return epoch;
    },
    schedule: (scheduledEpoch, callback) => {
      if (disposed) {
        return cancelledFrame;
      }

      const pendingFrame = {
        epoch: scheduledEpoch,
        frame: cancelledFrame,
        cancelled: false
      } satisfies PendingFrame;
      pending.add(pendingFrame);
      pendingFrame.frame = scheduleTimelineFrame((timestamp) => {
        pending.delete(pendingFrame);
        if (!pendingFrame.cancelled && !disposed && scheduledEpoch === epoch) {
          callback(timestamp);
        }
      });
      return {
        cancel: () => cancel(pendingFrame)
      };
    },
    cancelBefore: (beforeEpoch) => {
      for (const pendingFrame of [...pending]) {
        if (pendingFrame.epoch < beforeEpoch) {
          cancel(pendingFrame);
        }
      }
    },
    dispose: () => {
      if (disposed) {
        return;
      }
      disposed = true;
      for (const pendingFrame of [...pending]) {
        cancel(pendingFrame);
      }
    }
  };
}

export interface ManualTimelineViewportScheduler extends TimelineViewportScheduler {
  flushNext(timestamp?: number): boolean;
  flushAll(timestamp?: number): number;
}

export function createManualTimelineViewportScheduler(): ManualTimelineViewportScheduler {
  let epoch = 0;
  let disposed = false;
  const queue: Array<{
    epoch: ViewportEpoch;
    callback: FrameRequestCallback;
    cancelled: boolean;
  }> = [];

  const cancel = (queuedFrame: (typeof queue)[number]) => {
    queuedFrame.cancelled = true;
  };

  const flushNext = (timestamp = 0): boolean => {
    while (queue.length > 0) {
      const queuedFrame = queue.shift();
      if (!queuedFrame) {
        return false;
      }
      if (!queuedFrame.cancelled) {
        if (!disposed && queuedFrame.epoch === epoch) {
          queuedFrame.callback(timestamp);
        }
        return true;
      }
    }
    return false;
  };

  return {
    currentEpoch: () => epoch,
    advance: () => {
      if (!disposed) {
        epoch += 1;
      }
      return epoch;
    },
    schedule: (scheduledEpoch, callback) => {
      if (disposed) {
        return cancelledFrame;
      }
      const queuedFrame = { epoch: scheduledEpoch, callback, cancelled: false };
      queue.push(queuedFrame);
      return { cancel: () => cancel(queuedFrame) };
    },
    cancelBefore: (beforeEpoch) => {
      for (const queuedFrame of queue) {
        if (queuedFrame.epoch < beforeEpoch) {
          cancel(queuedFrame);
        }
      }
    },
    dispose: () => {
      disposed = true;
      queue.length = 0;
    },
    flushNext,
    flushAll: (timestamp = 0) => {
      let flushed = 0;
      while (flushNext(timestamp)) {
        flushed += 1;
        if (flushed >= MAX_MANUAL_FLUSHES) {
          throw new Error("timeline viewport manual scheduler did not settle");
        }
      }
      return flushed;
    }
  };
}
