import { describe, expect, it, vi } from "vitest";

import { createManualTimelineViewportScheduler } from "./TimelineViewportScheduler";

describe("manual timeline viewport scheduler", () => {
  it("runs work scheduled for the current epoch", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const callback = vi.fn();

    scheduler.schedule(scheduler.currentEpoch(), callback);
    scheduler.flushAll();

    expect(callback).toHaveBeenCalledOnce();
    expect(callback).toHaveBeenCalledWith(0);
  });

  it("makes queued work from an older epoch inert", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const callback = vi.fn();
    const epoch = scheduler.currentEpoch();

    scheduler.schedule(epoch, callback);
    expect(scheduler.advance()).toBe(epoch + 1);
    scheduler.flushNext();

    expect(callback).not.toHaveBeenCalled();
  });

  it("cancels work scheduled before the given epoch", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const callback = vi.fn();
    const epoch = scheduler.currentEpoch();

    scheduler.schedule(epoch, callback);
    scheduler.cancelBefore(epoch + 1);
    scheduler.flushAll();

    expect(callback).not.toHaveBeenCalled();
  });

  it("makes follow-up frames with a captured epoch inert after advancement", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const followUp = vi.fn();
    const epoch = scheduler.currentEpoch();
    const first = vi.fn(() => {
      scheduler.schedule(epoch, followUp);
    });

    scheduler.schedule(epoch, first);
    scheduler.flushNext();
    scheduler.advance();
    scheduler.flushAll();

    expect(first).toHaveBeenCalledOnce();
    expect(followUp).not.toHaveBeenCalled();
  });

  it("drops a stale top-pagination anchor restore after user intent", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const viewport = { scrollTop: 240 };
    const paginationEpoch = scheduler.currentEpoch();

    scheduler.schedule(paginationEpoch, () => {
      viewport.scrollTop = 80;
    });
    scheduler.advance();
    viewport.scrollTop = 360;
    scheduler.flushAll();

    expect(viewport.scrollTop).toBe(360);
  });

  it("uses one advance for room or projection replacement", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const oldRoomWork = vi.fn();
    const newProjectionWork = vi.fn();
    const oldEpoch = scheduler.currentEpoch();

    scheduler.schedule(oldEpoch, oldRoomWork);
    const newEpoch = scheduler.advance();
    scheduler.schedule(newEpoch, newProjectionWork);
    scheduler.flushAll();

    expect(oldRoomWork).not.toHaveBeenCalled();
    expect(newProjectionWork).toHaveBeenCalledOnce();
  });

  it("bounds recursively scheduled manual work", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const repeat = () => scheduler.schedule(scheduler.currentEpoch(), repeat);

    scheduler.schedule(scheduler.currentEpoch(), repeat);

    expect(() => scheduler.flushAll()).toThrow(
      "timeline viewport manual scheduler did not settle"
    );
  });

  it("makes queued and future work inert after disposal", () => {
    const scheduler = createManualTimelineViewportScheduler();
    const queued = vi.fn();
    const future = vi.fn();

    scheduler.schedule(scheduler.currentEpoch(), queued);
    scheduler.dispose();
    scheduler.schedule(scheduler.currentEpoch(), future);
    scheduler.flushAll();

    expect(queued).not.toHaveBeenCalled();
    expect(future).not.toHaveBeenCalled();
  });
});
