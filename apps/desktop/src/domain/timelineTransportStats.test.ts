import { beforeEach, describe, expect, test } from "vitest";

import {
  getTimelineTransportStats,
  recordTimelineEventReceived,
  recordTimelineInitialItems,
  recordTimelineKeyMismatch,
  recordTimelineResync,
  resetTimelineTransportStats
} from "./timelineTransportStats";

describe("timeline transport stats", () => {
  beforeEach(() => {
    resetTimelineTransportStats();
  });

  test("starts at zero", () => {
    expect(getTimelineTransportStats()).toEqual({
      received: 0,
      keyMismatchDropped: 0,
      keyMismatchGroups: {},
      initialItemsApplied: 0,
      lastInitialItemsCount: 0,
      resync: 0
    });
  });

  test("counts received events, key-mismatch drops, applied initial items, and resyncs", () => {
    recordTimelineEventReceived();
    recordTimelineEventReceived();
    expect(recordTimelineKeyMismatch("room", "thread", true, true, 1_000)).toBe(true);
    recordTimelineInitialItems(42);
    recordTimelineResync();

    expect(getTimelineTransportStats()).toEqual({
      received: 2,
      keyMismatchDropped: 1,
      keyMismatchGroups: { "room:thread:account_match:room_match": 1 },
      initialItemsApplied: 1,
      lastInitialItemsCount: 42,
      resync: 1
    });
  });

  test("aggregates a 5,000-event mismatch burst with one rate-limited summary", () => {
    let summaries = 0;
    for (let index = 0; index < 5_000; index += 1) {
      summaries += Number(
        recordTimelineKeyMismatch("room", "focused", true, false, 10_000)
      );
    }

    expect(summaries).toBe(1);
    expect(getTimelineTransportStats().keyMismatchGroups).toEqual({
      "room:focused:account_match:room_mismatch": 5_000
    });
    expect(recordTimelineKeyMismatch("room", "focused", true, false, 40_000)).toBe(true);
  });

  test("keeps the most recent initial-items count", () => {
    recordTimelineInitialItems(10);
    recordTimelineInitialItems(0);

    const stats = getTimelineTransportStats();
    expect(stats.initialItemsApplied).toBe(2);
    expect(stats.lastInitialItemsCount).toBe(0);
  });
});
