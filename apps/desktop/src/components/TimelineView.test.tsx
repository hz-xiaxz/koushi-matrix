// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode, Suspense, startTransition, useEffect, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { openExternalHttpUrl } from "../domain/externalLinks";

vi.mock("../domain/externalLinks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../domain/externalLinks")>()),
  openExternalHttpUrl: vi.fn(async () => undefined)
}));

import {
  focusedTimelineKey,
  roomTimelineKey,
  threadTimelineKey,
  type CoreEventPayload,
  type TimelineGapId,
  type TimelineItem,
} from "../domain/coreEvents";
import { setActiveLocaleProfile } from "../i18n/messages";
import {
  KEY,
  baseTransport,
  fileMessage,
  imageMessage,
  message,
  mockTimelineRects,
  navigationSnapshot
} from "./timelineViewTestSupport";
import {
  applyTimelineEvent,
  createTimelineStore,
  type TimelineStoreState
} from "../domain/timelineStore";
import { TimelineStoreContext } from "./timelineStoreContext";
import {
  TimelineView,
  clearTimelineViewportSessionMemoryForTests,
  timelineMediaDisplayBoxForTests
} from "./TimelineView";
import type {
  LiveSignalsState,
  TimelineContinuityState
} from "../domain/types";
import type {
  RoomKeyRequestStateDto,
  RoomKeyRequestWithheldCode
} from "../domain/coreEvents";
import { resetTimelineTransportStats } from "../domain/timelineTransportStats";

afterEach(() => {
  cleanup();
  clearTimelineViewportSessionMemoryForTests();
  setActiveLocaleProfile("en", "none");
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/**
 * Gives every rendered timeline row a deterministic block position based on
 * its presentation order. Unlike `mockTimelineRects`, this intentionally
 * follows DOM reordering so a test can observe the viewport correction that a
 * display-projection transaction must make.
 */
function mockPresentationOrderRects(
  scrollContainerRef: { current: HTMLElement | null },
  options: { rowHeight?: number; viewportHeight?: number } = {}
) {
  const rowHeight = options.rowHeight ?? 100;
  const viewportHeight = options.viewportHeight ?? 200;
  return vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement
  ) {
    const testId = this.getAttribute("data-testid");
    if (testId === "timeline-view") {
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: 0,
        width: 0,
        height: viewportHeight,
        bottom: viewportHeight,
        toJSON: () => ({})
      } as DOMRect;
    }

    const row = this.matches("article[data-item-id]")
      ? this
      : this.querySelector<HTMLElement>("article[data-item-id]");
    if (!row) {
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: 0,
        width: 0,
        height: 0,
        bottom: 0,
        toJSON: () => ({})
      } as DOMRect;
    }
    const rows = Array.from(document.querySelectorAll<HTMLElement>("article[data-item-id]"));
    const index = rows.indexOf(row);
    const top = index * rowHeight - (scrollContainerRef.current?.scrollTop ?? 0);
    return {
      x: 0,
      y: top,
      top,
      left: 0,
      right: 0,
      width: 0,
      height: rowHeight,
      bottom: top + rowHeight,
      toJSON: () => ({})
    } as DOMRect;
  });
}

describe("TimelineView", () => {

  it("computes a stable clamped media box for known image dimensions", () => {
    expect(timelineMediaDisplayBoxForTests(2048, 1188)).toEqual({
      inlineSize: 420,
      blockSize: 244
    });
    expect(timelineMediaDisplayBoxForTests(800, 1600)).toEqual({
      inlineSize: 130,
      blockSize: 260
    });
    expect(timelineMediaDisplayBoxForTests(null, 1600)).toEqual({
      inlineSize: 347,
      blockSize: 260
    });
    expect(timelineMediaDisplayBoxForTests(800, null)).toEqual({
      inlineSize: 347,
      blockSize: 260
    });
  });

  it("omits reply in thread from focused presentation while preserving ordinary reply", () => {
    const key = focusedTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$focused:example.invalid"
    );
    const store: TimelineStoreState = applyTimelineEvent(createTimelineStore(), {
      InitialItems: {
        request_id: null,
        key,
        generation: 1,
        items: [message("$focused-reply", "focused reply")]
      }
    });

    render(
      <TimelineStoreContext.Provider value={{ store, setStore: vi.fn() }}>
        <TimelineView
          presentationContext="focused"
          timelineKey={key}
          roomId="!room:example.invalid"
          transport={baseTransport({})}
          onReply={vi.fn()}
          onOpenThread={vi.fn()}
        />
      </TimelineStoreContext.Provider>
    );

    const row = screen.getByText("focused reply").closest("article");
    expect(row).not.toBeNull();
    expect(within(row!).getByRole("button", { name: "Reply to message" })).not.toBeNull();
    expect(within(row!).queryByRole("button", { name: "Reply in thread" })).toBeNull();
  });

  it("omits every reply-composition affordance from thread presentation", () => {
    const key = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$thread-root:example.invalid"
    );
    const onOpenContextMenu = vi.fn();
    const store: TimelineStoreState = applyTimelineEvent(createTimelineStore(), {
      InitialItems: {
        request_id: null,
        key,
        generation: 1,
        items: [
          {
            ...message("$thread-reply", "thread reply"),
            thread_root: "$thread-root:example.invalid",
            thread_summary: {
              reply_count: 2,
              latest_event_id: "$thread-latest:example.invalid",
              latest_sender: "@bob:example.invalid",
              latest_sender_label: null,
              latest_body_preview: "Latest",
              latest_timestamp_ms: 1_800_000_000_100
            }
          }
        ]
      }
    });

    render(
      <TimelineStoreContext.Provider value={{ store, setStore: vi.fn() }}>
        <TimelineView
          presentationContext="thread"
          timelineKey={key}
          roomId="!room:example.invalid"
          currentUserId="@alice:example.invalid"
          transport={baseTransport({})}
          onReply={vi.fn()}
          onOpenThread={vi.fn()}
          onOpenContextMenu={onOpenContextMenu}
        />
      </TimelineStoreContext.Provider>
    );

    const row = screen.getByText("thread reply").closest("article");
    expect(row).not.toBeNull();
    expect(within(row!).queryByRole("button", { name: "Reply to message" })).toBeNull();
    expect(within(row!).queryByRole("button", { name: "Reply in thread" })).toBeNull();

    fireEvent.contextMenu(row!);
    expect(onOpenContextMenu).toHaveBeenCalledTimes(1);
    const menuItems = onOpenContextMenu.mock.calls[0][2] as Array<{ id: string }>;
    expect(menuItems.map((item) => item.id)).not.toContain("replyToMessage");
    expect(menuItems.map((item) => item.id)).not.toContain("openThread");
    // The menu still has to be useful for the remaining thread-event actions.
    expect(menuItems.length).toBeGreaterThan(0);
  });

  it("renders an incoming rich reply quote inside thread presentation", () => {
    const key = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$thread-root:example.invalid"
    );
    const store: TimelineStoreState = applyTimelineEvent(createTimelineStore(), {
      InitialItems: {
        request_id: null,
        key,
        generation: 1,
        items: [
          {
            ...message("$thread-rich-reply", "Rich reply from another client"),
            thread_root: "$thread-root:example.invalid",
            in_reply_to_event_id: "$thread-earlier:example.invalid",
            reply_quote: {
              event_id: "$thread-earlier:example.invalid",
              sender: "@bob:example.invalid",
              sender_label: "Bob",
              body_preview: "Earlier thread event",
              state: "ready"
            }
          }
        ]
      }
    });

    render(
      <TimelineStoreContext.Provider value={{ store, setStore: vi.fn() }}>
        <TimelineView
          presentationContext="thread"
          timelineKey={key}
          roomId="!room:example.invalid"
          transport={baseTransport({})}
          onReply={vi.fn()}
          onOpenThread={vi.fn()}
        />
      </TimelineStoreContext.Provider>
    );

    const row = screen.getByText("Rich reply from another client").closest("article");
    expect(row).not.toBeNull();
    const quote = row!.querySelector<HTMLElement>(".reply-quote");
    expect(quote?.getAttribute("data-reply-state")).toBe("ready");
    expect(quote?.textContent).toContain("Bob");
    expect(quote?.textContent).toContain("Earlier thread event");
  });

  it("marks the latest visible room event as read even when bottom pixels are not exact", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const sendReadReceipt = vi.fn().mockResolvedValue(undefined);
    const setFullyRead = vi.fn().mockResolvedValue(undefined);
    const observeViewport = vi.fn().mockResolvedValue(undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      sendReadReceipt,
      setFullyRead,
      observeViewport
    });
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectSpy = mockTimelineRects(
      {
        "$older:example.invalid": { top: 40, height: 80 },
        "$latest:example.invalid": { top: 140, height: 80 }
      },
      { top: 0, height: 500 },
      scrollContainerRef
    );

    try {
      render(
        <TimelineView
          timelineKey={KEY}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
        />
      );

      const timeline = await screen.findByTestId("timeline-view");
      scrollContainerRef.current = timeline;
      Object.defineProperty(timeline, "clientHeight", { value: 500, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 2_000, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: KEY,
              generation: 1,
              items: [
                message("$older:example.invalid", "Older visible message"),
                message("$latest:example.invalid", "Latest visible message")
              ]
            }
          }
        });
      });

      timeline.scrollTop = 0;
      fireEvent.wheel(timeline, { deltaY: 1 });
      fireEvent.scroll(timeline);

      await waitFor(() => {
        expect(sendReadReceipt).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$latest:example.invalid"
        );
      });
      expect(setFullyRead).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$latest:example.invalid"
      );
      expect(observeViewport).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$older:example.invalid",
        "$latest:example.invalid",
        [],
        true
      );
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("marks the latest visible thread event with a threaded read receipt", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$root:example.invalid"
    );
    const sendReadReceipt = vi.fn().mockResolvedValue(undefined);
    const setFullyRead = vi.fn().mockResolvedValue(undefined);
    const observeViewport = vi.fn().mockResolvedValue(undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      sendReadReceipt,
      setFullyRead,
      observeViewport
    });
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectSpy = mockTimelineRects(
      {
        "$thread-reply:example.invalid": { top: 140, height: 80 }
      },
      { top: 0, height: 500 },
      scrollContainerRef
    );

    try {
      render(
        <TimelineView
          timelineKey={threadKey}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
        />
      );

      const timeline = await screen.findByTestId("timeline-view");
      scrollContainerRef.current = timeline;
      Object.defineProperty(timeline, "clientHeight", { value: 500, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 2_000, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: threadKey,
              generation: 1,
              items: [message("$thread-reply:example.invalid", "Thread reply")]
            }
          }
        });
      });

      timeline.scrollTop = 0;
      fireEvent.wheel(timeline, { deltaY: 1 });
      fireEvent.scroll(timeline);

      await waitFor(() => {
        expect(sendReadReceipt).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$thread-reply:example.invalid",
          "$root:example.invalid"
        );
      });
      expect(setFullyRead).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$thread-reply:example.invalid"
      );
      expect(observeViewport).not.toHaveBeenCalled();
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("preserves gap identity when the same thread root crosses it in latestReply mode", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const observeViewport = vi.fn().mockResolvedValue(undefined);
    const fullRangeGapId = {
      topology_revision: "14695981039346656037",
      ordinal: 0
    };
    const rootEvent = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_010_000
      }
    };
    const latestReply = {
      ...message("$thread-reply:example.invalid", "Standalone thread reply"),
      timestamp_ms: 1_800_000_010_000,
      thread_root: "$thread-root:example.invalid"
    };
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectSpy = mockTimelineRects(
      {
        "$before:example.invalid": { top: -200, height: 40 },
        "$thread-root:example.invalid": { top: 40, height: 40 },
        "$between:example.invalid": { top: -200, height: 40 },
        "$thread-reply:example.invalid": { top: 160, height: 40 },
        "$after:example.invalid": { top: 800, height: 40 },
        "timeline-gap-row": { top: 100, height: 40 }
      },
      { top: 0, height: 500 },
      scrollContainerRef
    );
    const transport = baseTransport({
      observeViewport,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    try {
      const view = (threadRootOrder: "rootEvent" | "latestReply") => (
        <TimelineView
          timelineKey={KEY}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
          threadRootOrder={{ kind: threadRootOrder }}
          continuity={{
            kind: "repairing",
            generation: 3,
            gap_count: 1,
            batches_processed: 0,
            minimum_batch_id: null
          }}
        />
      );
      const { rerender } = render(view("rootEvent"));

      const timeline = await screen.findByTestId("timeline-view");
      scrollContainerRef.current = timeline;
      Object.defineProperty(timeline, "clientHeight", { value: 500, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 2_000, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: KEY,
              generation: 1,
              items: [
                message("$before:example.invalid", "Before"),
                rootEvent,
                message("$between:example.invalid", "Between"),
                latestReply,
                message("$after:example.invalid", "After")
              ]
            }
          }
        });
        emit({
          kind: "Timeline",
          event: {
            GapPositionsUpdated: {
              key: KEY,
              actor_generation: 0,
              generation: 3,
              positions: [
                {
                  id: fullRangeGapId,
                  before_item_index: 3
                }
              ]
            }
          }
        });
      });

      timeline.scrollTop = 0;
      fireEvent.wheel(timeline, { deltaY: 1 });
      fireEvent.scroll(timeline);

      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$thread-root:example.invalid",
          "$thread-root:example.invalid",
          [fullRangeGapId],
          false
        );
      });
      const gap = screen.getByTestId("timeline-gap-row");
      const root = screen.getByText("Thread root").closest<HTMLElement>("article");
      expect(root).not.toBeNull();
      expect(root?.dataset["rowId"]).toBe("thread-root:$thread-root:example.invalid");
      expect(root?.dataset["contentEventId"]).toBe("$thread-root:example.invalid");
      expect(root?.dataset["activityEventId"]).toBe("$thread-root:example.invalid");
      expect(root!.compareDocumentPosition(gap) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
      expect(gap.dataset["gapTopologyRevision"]).toBe(fullRangeGapId.topology_revision);
      expect(gap.dataset["gapOrdinal"]).toBe(String(fullRangeGapId.ordinal));

      observeViewport.mockClear();
      rerender(view("latestReply"));
      fireEvent.scroll(timeline);

      await waitFor(() => {
        const movedRoot = screen.getByText("Thread root").closest<HTMLElement>("article");
        expect(movedRoot).toBe(root);
        expect(gap.compareDocumentPosition(movedRoot!) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(
          0
        );
        expect(movedRoot?.dataset["activityEventId"]).toBe("$thread-reply:example.invalid");
      });
      expect(screen.getByTestId("timeline-gap-row")).toBe(gap);
      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$thread-reply:example.invalid",
          "$thread-reply:example.invalid",
          [fullRangeGapId],
          false
        );
      });
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("covers selected-room persisted gap recovery through live history and room switch", async () => {
    let releaseRepairAcknowledgement: () => void = () => undefined;
    const pendingRepairAcknowledgement = new Promise<void>((resolve) => {
      releaseRepairAcknowledgement = resolve;
    });
    const acknowledgeRenderedBatch = vi.fn(() => pendingRepairAcknowledgement);
    const observeViewport = vi.fn().mockResolvedValue(undefined);
    const gapId = { topology_revision: "14695981039346656037", ordinal: 0 };
    const otherRoomId = "!other-room:example.invalid";
    const otherKey = roomTimelineKey("@alice:example.invalid", otherRoomId);
    const rootEvent = {
      ...message("$persisted-thread-root:example.invalid", "Persisted thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$persisted-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest persisted reply",
        latest_timestamp_ms: 1_800_000_010_000
      }
    };
    const latestReply = {
      ...message("$persisted-thread-reply:example.invalid", "Standalone persisted reply"),
      timestamp_ms: 1_800_000_010_000,
      thread_root: "$persisted-thread-root:example.invalid"
    };
    const liveEvent = message("$persisted-live:example.invalid", "New live event");
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectSpy = mockTimelineRects(
      {
        "$persisted-before:example.invalid": { top: -200, height: 40 },
        "$persisted-thread-root:example.invalid": { top: 40, height: 40 },
        "$persisted-between:example.invalid": { top: -200, height: 40 },
        "$persisted-thread-reply:example.invalid": { top: 160, height: 40 },
        "$persisted-live:example.invalid": { top: 600, height: 40 },
        "$other-room-event:example.invalid": { top: 40, height: 40 },
        "timeline-gap-row": { top: 100, height: 40 }
      },
      { top: 0, height: 500 },
      scrollContainerRef
    );
    const transport = baseTransport({ acknowledgeRenderedBatch, observeViewport });
    const repairing = {
      kind: "repairing" as const,
      generation: 31,
      gap_count: 1,
      batches_processed: 0,
      minimum_batch_id: null
    };
    let store = applyTimelineEvent(createTimelineStore(), {
      InitialItems: {
        request_id: null,
        key: KEY,
        actor_generation: 0,
        generation: 1,
        items: [
          message("$persisted-before:example.invalid", "Before persisted gap"),
          rootEvent,
          message("$persisted-between:example.invalid", "Between root and gap"),
          latestReply
        ]
      }
    });
    store = applyTimelineEvent(store, {
      GapPositionsUpdated: {
        key: KEY,
        actor_generation: 0,
        generation: 31,
        positions: [{ id: gapId, before_item_index: 3 }]
      }
    });
    const setStore = vi.fn();
    const view = (
      timelineKey: typeof KEY,
      roomId: string,
      order: "rootEvent" | "latestReply",
      continuity: TimelineContinuityState
    ) => (
      <TimelineView
        timelineKey={timelineKey}
        roomId={roomId}
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: order }}
        continuity={continuity}
        timelineStore={store}
        setTimelineStore={setStore}
      />
    );
    const oldRoomGapObservations = () =>
      observeViewport.mock.calls.filter(
        ([roomId, , , visibleGapIds]) =>
          roomId === "!room:example.invalid" &&
          (visibleGapIds as TimelineGapId[]).some(
            (id) =>
              id.topology_revision === gapId.topology_revision && id.ordinal === gapId.ordinal
          )
      );

    try {
      const { rerender } = render(view(KEY, "!room:example.invalid", "rootEvent", repairing));
      const timeline = await screen.findByTestId("timeline-view");
      scrollContainerRef.current = timeline;
      Object.defineProperty(timeline, "clientHeight", { value: 500, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });
      // Mount observations happen before jsdom receives stable dimensions.
      observeViewport.mockClear();

      fireEvent.wheel(timeline, { deltaY: 1 });
      fireEvent.scroll(timeline);
      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$persisted-thread-root:example.invalid",
          "$persisted-thread-root:example.invalid",
          [gapId],
          false
        );
      });
      const gap = screen.getByTestId("timeline-gap-row");
      const root = screen.getByText("Persisted thread root").closest<HTMLElement>("article");
      expect(root).not.toBeNull();
      expect(root!.compareDocumentPosition(gap) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
      expect(oldRoomGapObservations()).toHaveLength(1);
      fireEvent.scroll(timeline);
      await act(async () => Promise.resolve());
      expect(oldRoomGapObservations()).toHaveLength(1);

      rerender(view(KEY, "!room:example.invalid", "latestReply", repairing));
      await waitFor(() => {
        const movedRoot = screen
          .getByText("Persisted thread root")
          .closest<HTMLElement>("article");
        expect(movedRoot).toBe(root);
        expect(screen.getByTestId("timeline-gap-row")).toBe(gap);
        expect(gap.compareDocumentPosition(movedRoot!) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(
          0
        );
        expect(movedRoot?.dataset["activityEventId"]).toBe(
          "$persisted-thread-reply:example.invalid"
        );
      });

      store = applyTimelineEvent(store, {
        ItemsUpdated: {
          key: KEY,
          generation: 1,
          batch_id: 6,
          diffs: [{ PushBack: { item: liveEvent } }]
        }
      });
      const repairingAfterBatch = {
        ...repairing,
        batches_processed: 1,
        minimum_batch_id: 6
      };
      rerender(view(KEY, "!room:example.invalid", "latestReply", repairingAfterBatch));
      const liveRow = await screen.findByText("New live event").then((node) =>
        node.closest<HTMLElement>("article")
      );
      expect(liveRow).not.toBeNull();
      expect(gap.compareDocumentPosition(liveRow!) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);

      timeline.scrollTop = 500;
      fireEvent.wheel(timeline, { deltaY: 1 });
      fireEvent.scroll(timeline);
      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$persisted-live:example.invalid",
          "$persisted-live:example.invalid",
          [],
          true
        );
        expect(acknowledgeRenderedBatch).toHaveBeenCalledWith(KEY, 0, 1, 31, 6);
      });
      expect(acknowledgeRenderedBatch).toHaveBeenCalledTimes(1);
      rerender(view(KEY, "!room:example.invalid", "latestReply", repairingAfterBatch));
      fireEvent.scroll(timeline);
      await act(async () => Promise.resolve());
      expect(acknowledgeRenderedBatch).toHaveBeenCalledTimes(1);

      rerender(view(KEY, "!room:example.invalid", "latestReply", repairingAfterBatch));
      timeline.scrollTop = 0;
      fireEvent.wheel(timeline, { deltaY: -1 });
      fireEvent.scroll(timeline);
      await waitFor(() => expect(oldRoomGapObservations()).toHaveLength(2));
      expect(screen.getByTestId("timeline-gap-row")).toBe(gap);
      expect(oldRoomGapObservations().at(-1)?.[3]).toEqual([gapId]);
      expect(oldRoomGapObservations().every((call) => call[3].length === 1)).toBe(true);

      act(() => releaseRepairAcknowledgement());
      await act(async () => pendingRepairAcknowledgement);
      store = applyTimelineEvent(store, {
        GapPositionsUpdated: {
          key: KEY,
          actor_generation: 0,
          generation: 32,
          positions: []
        }
      });
      rerender(view(KEY, "!room:example.invalid", "latestReply", repairingAfterBatch));
      await waitFor(() => expect(screen.queryByTestId("timeline-gap-row")).toBeNull());
      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          "!room:example.invalid",
          "$persisted-thread-reply:example.invalid",
          "$persisted-thread-reply:example.invalid",
          [],
          false
        );
      });

      const oldGapObservationCount = oldRoomGapObservations().length;
      store = applyTimelineEvent(store, {
        InitialItems: {
          request_id: null,
          key: otherKey,
          actor_generation: 10,
          generation: 1,
          items: [message("$other-room-event:example.invalid", "Other room event")]
        }
      });
      store = applyTimelineEvent(store, {
        GapPositionsUpdated: {
          key: KEY,
          actor_generation: 0,
          generation: 33,
          positions: [{ id: gapId, before_item_index: 3 }]
        }
      });
      rerender(
        view(otherKey, otherRoomId, "rootEvent", {
          kind: "healthy",
          generation: 1,
          authoritative_start: false
        })
      );
      timeline.scrollTop = 0;
      fireEvent.scroll(timeline);
      await waitFor(() => {
        expect(observeViewport).toHaveBeenCalledWith(
          otherRoomId,
          "$other-room-event:example.invalid",
          "$other-room-event:example.invalid",
          [],
          true
        );
      });
      expect(screen.queryByTestId("timeline-gap-row")).toBeNull();
      expect(oldRoomGapObservations()).toHaveLength(oldGapObservationCount);
    } finally {
      releaseRepairAcknowledgement();
      rectSpy.mockRestore();
    }
  });

  it("emits safe timestamped timeline event diagnostics for thread timelines", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onDiagnosticLogEntry = vi.fn();
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$root:example.invalid"
    );
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: threadKey,
          generation: 3,
          items: [message("$root:example.invalid", "Thread root")]
        }
      }
    });
    emit({
      kind: "Timeline",
      event: {
        PaginationStateChanged: {
          request_id: null,
          key: threadKey,
          direction: "Backward",
          state: "EndReached"
        }
      }
    });

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.event",
          message: "kind=thread initial items=1 generation=3"
        })
      );
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.event",
          message: "kind=thread pagination direction=Backward state=EndReached"
        })
      );
    });
    expect(onDiagnosticLogEntry.mock.calls.map(([entry]) => entry.message).join("\n")).not.toContain(
      "$root"
    );
  });

  it("emits privacy-safe focused store lookup and event-key mismatch diagnostics", async () => {
    resetTimelineTransportStats();
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onDiagnosticLogEntry = vi.fn();
    const focusedKey = focusedTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$target:example.invalid"
    );
    const otherKey = focusedTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$other:example.invalid"
    );
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={focusedKey}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
        timelineStore={createTimelineStore()}
      />
    );

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.store",
          message: expect.stringContaining(
            "stage=lookup kind=focused"
          ) as unknown as string
        })
      );
    });

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: { connection_id: 9, sequence: 1 },
          key: otherKey,
          actor_generation: 1,
          generation: 1,
          items: []
        }
      }
    });

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.key",
          message: expect.stringContaining(
            "stage=event_dropped_summary current_kind=focused event_kind=focused"
          ) as unknown as string
        })
      );
    });
    const diagnostics = onDiagnosticLogEntry.mock.calls
      .map(([entry]) => `${entry.source} ${entry.message}`)
      .join("\n");
    expect(diagnostics).toContain("account_match=true");
    expect(diagnostics).toContain("room_match=true");
    expect(diagnostics).not.toContain("@alice:example.invalid");
    expect(diagnostics).not.toContain("!room:example.invalid");
    expect(diagnostics).not.toContain("$target:example.invalid");
    expect(diagnostics).not.toContain("$other:example.invalid");
  });

  it("centers the focused target instead of restoring the focused window to live edge", async () => {
    const originalScrollIntoView = Element.prototype.scrollIntoView;
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      let emit: (payload: CoreEventPayload) => void = () => undefined;
      const onDiagnosticLogEntry = vi.fn();
      const focusedKey = focusedTimelineKey(
        "@alice:example.invalid",
        "!room:example.invalid",
        "$focused-target:example.invalid"
      );
      const transport = baseTransport({
        listenCoreEvents(nextListener) {
          emit = nextListener;
          return () => undefined;
        }
      });

      render(
        <TimelineView
          timelineKey={focusedKey}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
          onDiagnosticLogEntry={onDiagnosticLogEntry}
        />
      );

      const timeline = screen.getByTestId("timeline-view");
      Object.defineProperty(timeline, "clientHeight", { value: 400, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 1_800, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: focusedKey,
              generation: 1,
              items: [
                message("$focused-older:example.invalid", "Older"),
                message("$focused-target:example.invalid", "Target"),
                message("$focused-newer:example.invalid", "Newer")
              ]
            }
          }
        });
      });

      await waitFor(() => expect(scrollIntoView).toHaveBeenCalledTimes(1));
      const targetRow = scrollIntoView.mock.instances[0] as HTMLElement | undefined;
      expect(targetRow?.getAttribute("data-activity-event-id")).toBe(
        "$focused-target:example.invalid"
      );
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.scroll",
          message: "stage=focused_target_restore path=dom target_present=true"
        })
      );
      expect(
        onDiagnosticLogEntry.mock.calls.some(
          ([entry]) =>
            entry.source === "timeline.scroll" &&
            entry.message.includes("stage=room_reentry_restore") &&
            entry.message.includes("path=live_edge")
        )
      ).toBe(false);
    } finally {
      Element.prototype.scrollIntoView = originalScrollIntoView;
    }
  });

  it("records a deduplicated committed thread projection", async () => {
    const onDiagnosticLogEntry = vi.fn();
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$root:example.invalid"
    );
    let store = applyTimelineEvent(createTimelineStore(), {
      InitialItems: {
        request_id: null,
        key: threadKey,
        actor_generation: 5,
        generation: 3,
        items: [message("$root:example.invalid", "Thread root")]
      }
    });
    store = applyTimelineEvent(store, {
      ItemsUpdated: {
        key: threadKey,
        generation: 3,
        batch_id: 7,
        diffs: [{ PushBack: { item: message("$reply:example.invalid", "Reply") } }]
      }
    });

    const view = (
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={baseTransport({})}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
        timelineStore={store}
      />
    );
    const { rerender } = render(view);

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "thread.timeline",
          message: "stage=committed actor=5 generation=3 batch=7 items=2"
        })
      );
    });
    rerender(view);
    expect(
      onDiagnosticLogEntry.mock.calls.filter(
        ([entry]) =>
          entry.source === "thread.timeline" &&
          entry.message === "stage=committed actor=5 generation=3 batch=7 items=2"
      )
    ).toHaveLength(1);
  });

  it("renders read receipts as a compact avatar stack without an inline text label", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        liveSignals={{
          presence: {},
          rooms: {
            "!room:example.invalid": {
              fully_read_event_id: null,
              typing_user_ids: [],
              typing_users: [],
              receipts_by_event: {
                "$seen": {
                  total_count: 2,
                  overflow_count: 0,
                  readers: [
                    {
                      user_id: "@ken:example.invalid",
                      display_name: "Ken Inayoshi",
                      original_display_label: "Ken Inayoshi",
                      avatar: null,
                      timestamp_ms: null
                    },
                    {
                      user_id: "@satoshi:example.invalid",
                      display_name: "Satoshi Terasaki",
                      original_display_label: "Satoshi Terasaki",
                      avatar: null,
                      timestamp_ms: null
                    }
                  ]
                }
              }
            }
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [message("$seen", "Seen message")]
          }
        }
      });
    });

    await waitFor(() => {
      const receipts = document.querySelector(".message-receipts");
      expect(receipts).not.toBeNull();
      expect(receipts?.textContent).toContain("KE");
      expect(receipts?.textContent).toContain("SA");
      expect(receipts?.textContent).not.toContain("Read by 2");
      expect(receipts?.getAttribute("aria-label")).toContain("Read by 2");
      expect(receipts?.getAttribute("title")).toBe("Ken Inayoshi\nSatoshi Terasaki");
    });
  });

  it("opens the reader popup in the floating layer so a clipped pane cannot cut it", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    // A narrow, overflow-clipped container stands in for the thread pane.
    const pane = document.createElement("div");
    pane.className = "thread-pane";
    pane.style.overflow = "hidden";
    pane.style.width = "320px";
    document.body.appendChild(pane);

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        presentationContext="thread"
        liveSignals={{
          presence: {},
          rooms: {
            "!room:example.invalid": {
              fully_read_event_id: null,
              typing_user_ids: [],
              typing_users: [],
              receipts_by_event: {
                "$seen": {
                  total_count: 1,
                  overflow_count: 0,
                  readers: [
                    {
                      user_id: "@ken:example.invalid",
                      display_name: "Ken Inayoshi",
                      original_display_label: "Ken Inayoshi",
                      avatar: null,
                      timestamp_ms: 1_800_000_000_000
                    }
                  ]
                }
              }
            }
          }
        }}
        onReply={vi.fn()}
      />,
      { container: pane }
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [message("$seen", "Seen message")]
          }
        }
      });
    });

    const receipts = await waitFor(() => {
      const node = pane.querySelector<HTMLElement>(".message-receipts");
      expect(node).not.toBeNull();
      return node!;
    });

    // Closed by default; the details are not a row-local always-rendered child.
    expect(document.querySelector('[role="tooltip"]')).toBeNull();

    fireEvent.focus(receipts);
    const tooltip = await waitFor(() => {
      const node = document.querySelector<HTMLElement>('[role="tooltip"]');
      expect(node).not.toBeNull();
      return node!;
    });
    expect(tooltip.textContent).toContain("Ken Inayoshi");
    // The popup must escape the clipped pane, so it cannot be a descendant.
    expect(pane.contains(tooltip)).toBe(false);
    expect(tooltip.parentElement).toBe(document.body);

    fireEvent.blur(receipts);
    await waitFor(() => {
      expect(document.querySelector('[role="tooltip"]')).toBeNull();
    });

    pane.remove();
  });

  it("places reactions and read receipts in one status row", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        liveSignals={{
          presence: {},
          rooms: {
            "!room:example.invalid": {
              fully_read_event_id: null,
              typing_user_ids: [],
              typing_users: [],
              receipts_by_event: {
                "$reacted-seen": {
                  total_count: 1,
                  overflow_count: 0,
                  readers: [
                    {
                      user_id: "@ken:example.invalid",
                      display_name: "Ken Inayoshi",
                      original_display_label: "Ken Inayoshi",
                      avatar: null,
                      timestamp_ms: null
                    }
                  ]
                }
              }
            }
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              {
                ...message("$reacted-seen", "Reacted and seen"),
                reactions: [
                  {
                    key: "✈️",
                    count: 1,
                    reacted_by_me: false,
                    my_reaction_event_id: null,
                    sender_preview: [
                      { user_id: "@ken:example.invalid", display_label: "Ken Alias" }
                    ]
                  }
                ]
              }
            ]
          }
        }
      });
    });

    await waitFor(() => {
      const reactions = document.querySelector(".message-reactions");
      const receipts = document.querySelector(".message-receipts");
      const statusRow = document.querySelector(".message-status-row");

      expect(reactions).not.toBeNull();
      expect(receipts).not.toBeNull();
      expect(statusRow).not.toBeNull();
      expect(reactions?.parentElement).toBe(statusRow);
      expect(receipts?.parentElement).toBe(statusRow);
      expect(Array.from(statusRow?.children ?? [])).toEqual([reactions, receipts]);
    });
  });

  it("automatically requests previews for encrypted image attachments", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadMedia = vi.fn(async () => undefined);
    const transport = baseTransport({
      downloadMedia,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$encrypted-image", true)]
          }
        }
      });
    });

    await waitFor(() => {
      expect(downloadMedia).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$encrypted-image"
      );
    });
  });

  it("renders ready image with image-first layout and hover download overlay", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "appmedia://synthetic-image",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$ready-image", false)]
          }
        }
      });
    });

    await waitFor(() => {
      const media = document.querySelector('[data-event-id="$ready-image"] .message-media');
      expect(media).not.toBeNull();
      expect(media?.getAttribute("data-download-state")).toBe("ready");
      // #163: image-first layout — the preview is the primary block. The
      // filename lives on the image (alt), not as text laid over the preview,
      // and download appears in the hover/focus action overlay.
      const image = media?.querySelector<HTMLImageElement>(".message-media-image");
      expect(image).not.toBeNull();
      expect(image?.getAttribute("alt")).toBe("photo.png");
      const actionButtons = Array.from(
        media?.querySelectorAll<HTMLButtonElement>(
          ".message-media-hover-actions .message-media-hover-action"
        ) ?? []
      );
      const actionLabels = actionButtons.map((button) => button.getAttribute("aria-label"));
      expect(actionLabels).toEqual(["Show media details for photo.png", "Download photo.png"]);
      const downloadButton = actionButtons.find(
        (button) => button.getAttribute("aria-label") === "Download photo.png"
      );
      expect(downloadButton).not.toBeNull();
      expect(downloadButton?.tagName).toBe("BUTTON");
      expect(media?.textContent).not.toContain("image/png");
      expect(media?.textContent).not.toContain("407 KB");
    });
  });

  it("renders ready file downloads as navigation-safe buttons", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-file": {
            kind: "ready",
            source_url: "asset://localhost/notes.pdf",
            width: null,
            height: null,
            mime_type: "application/pdf"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [fileMessage("$ready-file")]
          }
        }
      });
    });

    await waitFor(() => {
      const downloadButton = document.querySelector<HTMLButtonElement>(
        '[data-event-id="$ready-file"] button.message-media-download'
      );
      expect(downloadButton).not.toBeNull();
      expect(downloadButton?.getAttribute("aria-label")).toBe("Download notes.pdf");
    });
  });

  it("routes ready image preview downloads through the transport when available", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const fetchMock = vi.fn(async () => new Response(new Blob(["image"], { type: "image/png" })));
    const createObjectURL = vi.fn(() => "blob:downloaded-image");
    const revokeObjectURL = vi.fn();
    const OriginalURL = URL;
    class MockURL extends OriginalURL {
      static override createObjectURL = createObjectURL;
      static override revokeObjectURL = revokeObjectURL;
    }
    const clickedAnchors: HTMLAnchorElement[] = [];
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("URL", MockURL);
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement
    ) {
      clickedAnchors.push(this);
    });
    const saveMediaFile = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      saveMediaFile
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "asset://localhost/original-photo.png",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$ready-image", false)]
          }
        }
      });
    });

    const downloadButton = await screen.findByRole("button", { name: "Download photo.png" });
    fireEvent.click(downloadButton);

    await waitFor(() => {
      expect(saveMediaFile).toHaveBeenCalledWith(
        "asset://localhost/original-photo.png",
        "photo.png"
      );
    });
    expect(fetchMock).not.toHaveBeenCalled();
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(clickedAnchors).toHaveLength(0);
    expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
  });

  it("does not request encrypted image previews for off-window initial virtualized items", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadMedia = vi.fn(async () => undefined);
    const transport = baseTransport({
      downloadMedia,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const items = Array.from({ length: 700 }, (_, index) =>
      index === 350
        ? imageMessage("$offscreen-image", true)
        : message(`$plain-${index}`, `Plain ${index}`)
    );

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items
          }
        }
      });
    });

    await waitFor(() => {
      const renderedItems = Number(
        screen.getByTestId("timeline-view").getAttribute("data-rendered-items")
      );
      expect(renderedItems).toBeGreaterThan(0);
      expect(renderedItems).toBeLessThan(items.length);
    });
    expect(downloadMedia).not.toHaveBeenCalledWith(
      "!room:example.invalid",
      "$offscreen-image"
    );
    expect(downloadMedia).not.toHaveBeenCalled();
  });

  it("opens ready image previews in an in-app media viewer", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "asset://localhost/original-photo.png",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$ready-image", true)]
          }
        }
      });
    });

    await waitFor(() => {
      const image = screen.getByRole("img", { name: "photo.png" });
      const previewButton = image.closest("button");
      expect(previewButton?.getAttribute("aria-label")).toBe("Open file");
      const media = document.querySelector(".message-media");
      // #163: image-first layout. The encrypted badge stays visible as a
      // security signal and the download sits in the hover overlay, but
      // filename/mimetype/size no longer occupy layout over the preview.
      expect(media?.querySelector(".message-media-image-badge")?.textContent).toContain(
        "Encrypted"
      );
      expect(media?.querySelector(".message-media-hover-actions")).not.toBeNull();
      expect(media?.textContent).not.toContain("image/png");
      expect(media?.textContent).not.toContain("407 KB");
    });

    fireEvent.click(screen.getByRole("button", { name: "Open file" }));

    const viewer = await screen.findByRole("dialog", { name: "Media viewer" });
    expect(viewer.textContent).toContain("photo.png");
    expect(viewer.textContent).toContain("407 KB");
    expect(viewer.querySelector<HTMLImageElement>(".timeline-media-viewer-image")?.src).toContain(
      "asset://localhost/original-photo.png"
    );

    fireEvent.click(screen.getByRole("button", { name: "Close media viewer" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
    });
  });

  it("keeps ready image metadata behind an inline details action", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "asset://localhost/original-photo.png",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$ready-image", true)]
          }
        }
      });
    });

    const detailsButton = await screen.findByRole("button", {
      name: "Show media details for photo.png"
    });
    const media = document.querySelector(".message-media");
    expect(media?.textContent).not.toContain("image/png");
    expect(media?.textContent).not.toContain("407 KB");

    fireEvent.click(detailsButton);

    const details = await screen.findByRole("dialog", { name: "Media details" });
    expect(details.textContent).toContain("photo.png");
    expect(details.textContent).toContain("image/png");
    expect(details.textContent).toContain("407 KB");
    expect(details.textContent).toContain("2048x1188");
    expect(details.textContent).toContain("Encrypted");
  });

  it("focuses the media viewer close control and returns focus to the clicked image", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "asset://localhost/original-photo.png",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [imageMessage("$ready-image", false)]
          }
        }
      });
    });

    const openButton = await screen.findByRole("button", { name: "Open file" });
    openButton.focus();
    fireEvent.click(openButton);

    const viewer = await screen.findByRole("dialog", { name: "Media viewer" });
    const closeButton = within(viewer).getByRole("button", { name: "Close media viewer" });
    await waitFor(() => {
      expect(document.activeElement).toBe(closeButton);
    });

    const tabEvent = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true
    });
    document.dispatchEvent(tabEvent);
    expect(tabEvent.defaultPrevented).toBe(true);
    expect(viewer.contains(document.activeElement)).toBe(true);

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
    });
    expect(document.activeElement).toBe(openButton);
  });

  it("routes media viewer message actions through the event transport", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const loadMessageSource = vi.fn(async () => undefined);
    const redactMessage = vi.fn(async () => undefined);
    const forwardMessage = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      loadMessageSource,
      redactMessage,
      forwardMessage
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        mediaDownloads={{
          "$ready-image": {
            kind: "ready",
            source_url: "asset://localhost/original-photo.png",
            width: 2048,
            height: 1188,
            mime_type: "image/png"
          }
        }}
        forwardDestinations={[
          {
            room_id: "!destination:example.invalid",
            display_name: "Destination room"
          }
        ]}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              {
                ...imageMessage("$ready-image", false),
                can_redact: true,
                actions: {
                  can_copy: false,
                  can_forward: true,
                  can_reply: true,
                  can_permalink: false,
                  can_view_source: true
                }
              }
            ]
          }
        }
      });
    });

    fireEvent.click(await screen.findByRole("button", { name: "Open file" }));
    let viewer = await screen.findByRole("dialog", { name: "Media viewer" });
    fireEvent.click(within(viewer).getByRole("button", { name: "Message actions" }));
    expect(within(viewer).getByRole("menu", { name: "Message actions" })).not.toBeNull();
    fireEvent.click(within(viewer).getByRole("menuitem", { name: "Forward" }));
    fireEvent.click(within(viewer).getByRole("menuitem", { name: "Destination room" }));
    await waitFor(() => {
      expect(forwardMessage).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$ready-image",
        "!destination:example.invalid"
      );
      expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
    });

    fireEvent.click(screen.getByRole("button", { name: "Open file" }));
    viewer = await screen.findByRole("dialog", { name: "Media viewer" });
    fireEvent.click(within(viewer).getByRole("button", { name: "Message actions" }));
    fireEvent.click(within(viewer).getByRole("menuitem", { name: "View source" }));
    await waitFor(() => {
      expect(loadMessageSource).toHaveBeenCalledWith("!room:example.invalid", "$ready-image");
      expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
    });

    fireEvent.click(screen.getByRole("button", { name: "Open file" }));
    viewer = await screen.findByRole("dialog", { name: "Media viewer" });
    fireEvent.click(within(viewer).getByRole("button", { name: "Message actions" }));
    fireEvent.click(within(viewer).getByRole("menuitem", { name: "Remove" }));

    await waitFor(() => {
      expect(redactMessage).toHaveBeenCalledWith("!room:example.invalid", "$ready-image");
      expect(screen.queryByRole("dialog", { name: "Media viewer" })).toBeNull();
    });
  });

  it("requests visible sender avatar thumbnails that are not yet downloaded", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadAvatarThumbnail = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      downloadAvatarThumbnail
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        enableAvatarThumbnailDownloads={true}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar", "Avatar row"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });

    await waitFor(() => {
      expect(downloadAvatarThumbnail).toHaveBeenCalledWith("mxc://matrix.org/avatar");
    });
    expect(downloadAvatarThumbnail).toHaveBeenCalledTimes(1);
  });

  it("limits initial avatar thumbnail requests to the current viewport window", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadAvatarThumbnail = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      downloadAvatarThumbnail
    });
    const items = Array.from({ length: 40 }, (_, index) => ({
      ...message(`$avatar-window-${index}`, `Avatar row ${index}`),
      sender_avatar: {
        mxc_uri: `mxc://matrix.org/avatar-window-${index}`,
        thumbnail: { kind: "notRequested" as const }
      }
    }));

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        enableAvatarThumbnailDownloads={true}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items
        }
      }
    });

    await waitFor(() => {
      expect(downloadAvatarThumbnail).toHaveBeenCalledWith(
        "mxc://matrix.org/avatar-window-0"
      );
    });
    expect(downloadAvatarThumbnail).not.toHaveBeenCalledWith(
      "mxc://matrix.org/avatar-window-39"
    );
    expect(downloadAvatarThumbnail.mock.calls.length).toBeLessThan(items.length);
  });

  it("emits timestamped avatar diagnostics for request, success, and retryable failure", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadAvatarThumbnail = vi.fn(async () => undefined);
    const onDiagnosticLogEntry = vi.fn();
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      downloadAvatarThumbnail
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
        enableAvatarThumbnailDownloads={true}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-retry", "Avatar row"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar-retry",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });

    await waitFor(() => {
      expect(downloadAvatarThumbnail).toHaveBeenCalledWith("mxc://matrix.org/avatar-retry");
    });
    expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "timeline.avatar",
        message: "avatar thumbnail request queued"
      })
    );

    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 3 },
          mxc_uri: "mxc://matrix.org/avatar-retry",
          thumbnail: {
            kind: "failed",
            request_id: 3,
            failureKind: "network"
          }
        }
      }
    });

    await waitFor(() => {
      expect(downloadAvatarThumbnail).toHaveBeenCalledTimes(2);
    });
    expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "timeline.avatar",
        message: "avatar thumbnail failed kind=network"
      })
    );

    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 4 },
          mxc_uri: "mxc://matrix.org/avatar-retry",
          thumbnail: {
            kind: "ready",
            source_url: "koushi-thumbnail://localhost/avatar/retry",
            width: null,
            height: null,
            mime_type: null
          }
        }
      }
    });

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.avatar",
          message: "avatar thumbnail ready"
        })
      );
    });
    expect(onDiagnosticLogEntry.mock.calls.every(([entry]) => Number.isFinite(entry.timestampMs)))
      .toBe(true);
  });

  it("requests profile avatar thumbnails when the timeline item has no sender avatar", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadAvatarThumbnail = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      downloadAvatarThumbnail
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        profileUsers={{
          "@bob:example.invalid": {
            user_id: "@bob:example.invalid",
            display_name: "Bob",
            display_label: "Bob",
            original_display_label: "Bob",
            mention_search_terms: ["bob"],
            avatar: {
              mxc_uri: "mxc://matrix.org/profile-avatar",
              thumbnail: { kind: "notRequested" }
            }
          }
        }}
        onReply={vi.fn()}
        enableAvatarThumbnailDownloads={true}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [message("$profile-avatar", "Profile avatar row")]
        }
      }
    });

    await waitFor(() => {
      expect(downloadAvatarThumbnail).toHaveBeenCalledWith("mxc://matrix.org/profile-avatar");
    });
    expect(downloadAvatarThumbnail).toHaveBeenCalledTimes(1);
  });

  it("does NOT call downloadAvatarThumbnail when enableAvatarThumbnailDownloads is explicitly false (kill-switch)", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const downloadAvatarThumbnail = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      downloadAvatarThumbnail
    });

    // Explicitly disable via the kill-switch prop (#116 Stage F1a: default is now ON).
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        enableAvatarThumbnailDownloads={false}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-gated", "Avatar row (kill-switch off)"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar-gated",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });

    // Give React time to flush any effects that might fire.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(downloadAvatarThumbnail).not.toHaveBeenCalled();
  });

  it("renders a downloaded sender avatar thumbnail from account events", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-ready", "Avatar row"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });
    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 2 },
          mxc_uri: "mxc://matrix.org/avatar",
          thumbnail: {
            kind: "ready",
            source_url: "koushi-thumbnail://localhost/avatar/sender",
            width: null,
            height: null,
            mime_type: null
          }
        }
      }
    });

    await waitFor(() => {
      const image = document.querySelector<HTMLImageElement>(".message .avatar img");
      expect(image?.getAttribute("src")).toBe("koushi-thumbnail://localhost/avatar/sender");
    });
  });

  it("ignores avatar thumbnail events that are not relevant to the mounted timeline", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onDiagnosticLogEntry = vi.fn();
    const onDiagnosticsChange = vi.fn();
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onDiagnosticsChange={onDiagnosticsChange}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-relevant", "Avatar row"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/relevant-avatar",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });
    await waitFor(() =>
      expect(onDiagnosticsChange).toHaveBeenCalledWith(
        expect.objectContaining({
          avatarMxcItems: 1,
          avatarPendingItems: 1,
          visibleItems: 1
        })
      )
    );
    onDiagnosticLogEntry.mockClear();
    onDiagnosticsChange.mockClear();

    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 2 },
          mxc_uri: "mxc://matrix.org/unrelated-avatar",
          thumbnail: {
            kind: "ready",
            source_url: "koushi-thumbnail://localhost/avatar/unrelated",
            width: null,
            height: null,
            mime_type: null
          }
        }
      }
    });

    await new Promise((resolve) => window.setTimeout(resolve, 0));
    expect(onDiagnosticLogEntry).not.toHaveBeenCalledWith(
      expect.objectContaining({
        source: "timeline.avatar",
        message: "avatar thumbnail ready"
      })
    );
    expect(onDiagnosticsChange).not.toHaveBeenCalled();
    expect(document.querySelector(".message .avatar img")).toBeNull();
  });

  it("renders downloaded thumbnails for multiple visible sender avatars", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-ready-a", "Avatar row A"),
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar-a",
                thumbnail: { kind: "notRequested" }
              }
            },
            {
              ...message("$avatar-ready-b", "Avatar row B"),
              sender: "@carol:example.invalid",
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar-b",
                thumbnail: { kind: "notRequested" }
              }
            }
          ]
        }
      }
    });
    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 2 },
          mxc_uri: "mxc://matrix.org/avatar-a",
          thumbnail: {
            kind: "ready",
            source_url: "koushi-thumbnail://localhost/avatar/a",
            width: null,
            height: null,
            mime_type: null
          }
        }
      }
    });
    emit({
      kind: "Account",
      event: {
        AvatarThumbnailDownloaded: {
          request_id: { connection_id: 1, sequence: 3 },
          mxc_uri: "mxc://matrix.org/avatar-b",
          thumbnail: {
            kind: "ready",
            source_url: "koushi-thumbnail://localhost/avatar/b",
            width: null,
            height: null,
            mime_type: null
          }
        }
      }
    });

    await waitFor(() => {
      const firstImage = document.querySelector<HTMLImageElement>(
        '[data-event-id="$avatar-ready-a"] .avatar img'
      );
      const secondImage = document.querySelector<HTMLImageElement>(
        '[data-event-id="$avatar-ready-b"] .avatar img'
      );
      expect(firstImage?.getAttribute("src")).toBe("koushi-thumbnail://localhost/avatar/a");
      expect(secondImage?.getAttribute("src")).toBe("koushi-thumbnail://localhost/avatar/b");
    });
  });

  it("falls back to sender initials when a downloaded sender avatar image is broken", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [
            {
              ...message("$avatar-broken", "Avatar row"),
              sender_label: "Ken Inayoshi",
              sender_avatar: {
                mxc_uri: "mxc://matrix.org/avatar-broken",
                thumbnail: {
                  kind: "ready",
                  source_url: "asset://missing-avatar.bin",
                  width: null,
                  height: null,
                  mime_type: null
                }
              }
            }
          ]
        }
      }
    });

    const image = await waitFor(() => {
      const element = document.querySelector<HTMLImageElement>(".message .avatar img");
      expect(element?.getAttribute("src")).toBe("asset://missing-avatar.bin");
      return element!;
    });
    fireEvent.error(image);

    expect(document.querySelector(".message .avatar img")).toBeNull();
    expect(document.querySelector(".message .avatar")?.textContent).toBe("KE");
  });

  it("retries a transiently broken sender avatar image URL", async () => {
    vi.useFakeTimers();
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              {
                ...message("$avatar-retry-render", "Avatar row"),
                sender_label: "Ken Inayoshi",
                sender_avatar: {
                  mxc_uri: "mxc://matrix.org/avatar-retry-render",
                  thumbnail: {
                    kind: "ready",
                    source_url: "asset://transient-avatar.bin",
                    width: null,
                    height: null,
                    mime_type: null
                  }
                }
              }
            ]
          }
        }
      });
    });

    const image = document.querySelector<HTMLImageElement>(".message .avatar img");
    expect(image).not.toBeNull();
    expect(image?.getAttribute("src")).toBe("asset://transient-avatar.bin");
    fireEvent.error(image!);
    expect(document.querySelector(".message .avatar img")).toBeNull();

    act(() => {
      vi.advanceTimersByTime(10_000);
    });

    expect(document.querySelector<HTMLImageElement>(".message .avatar img")?.getAttribute("src")).toBe(
      "asset://transient-avatar.bin"
    );
  });

  it("jumps to an unread event outside the mounted virtual window", async () => {
    const originalScrollIntoView = Element.prototype.scrollIntoView;
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      let emit: (payload: CoreEventPayload) => void = () => undefined;
      const transport = baseTransport({
        listenCoreEvents(nextListener) {
          emit = nextListener;
          return () => undefined;
        }
      });
      const items = Array.from({ length: 650 }, (_, index) =>
        message(`$virtual-${index}:example.invalid`, `Virtual message ${index}`)
      );

      render(
        <TimelineView
          timelineKey={KEY}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
        />
      );

      const timeline = await screen.findByTestId("timeline-view");
      Object.defineProperty(timeline, "clientHeight", { value: 500, configurable: true });
      Object.defineProperty(timeline, "scrollHeight", { value: 650 * 72, configurable: true });
      Object.defineProperty(timeline, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true
      });

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: KEY,
              generation: 1,
              items
            }
          }
        });
        emit({
          kind: "Timeline",
          event: {
            NavigationUpdated: {
              key: KEY,
              snapshot: {
                can_jump_to_bottom: false,
                first_unread_event_id: "$virtual-500:example.invalid",
                newer_event_count: 0,
                read_marker_display_event_id: null,
                read_marker_event_id: null,
                unread_event_count: 3,
                unread_position: "belowViewport"
              }
            }
          }
        });
      });

      await waitFor(() => {
        expect(timeline.getAttribute("data-virtualized")).toBe("true");
        expect(screen.getByRole("button", { name: /Jump to first unread/ })).toBeTruthy();
        expect(document.querySelector('[data-event-id="$virtual-500:example.invalid"]')).toBeNull();
      });

      fireEvent.click(screen.getByRole("button", { name: /Jump to first unread/ }));

      expect(timeline.scrollTop).toBeGreaterThan(30_000);
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    } finally {
      Element.prototype.scrollIntoView = originalScrollIntoView;
    }
  });

  it("backfills an empty thread timeline even when the first Core generation is zero", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$root:example.invalid"
    );
    const paginateBackwards = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      paginateBackwards
    });

    render(
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={transport}
        autoLoadOlderMessages
        onReply={vi.fn()}
      />
    );
    const timeline = screen.getByTestId("timeline-view");
    Object.defineProperty(timeline, "clientHeight", {
      value: 600,
      configurable: true
    });
    Object.defineProperty(timeline, "scrollHeight", {
      value: 0,
      configurable: true
    });

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: threadKey,
            generation: 0,
            items: []
          }
        }
      });
    });

    await waitFor(() => {
      expect(paginateBackwards).toHaveBeenCalledWith(threadKey);
    });
    expect(paginateBackwards).toHaveBeenCalledTimes(1);
  });

  it("keeps a new-thread draft out of backfill and hides stale pagination state", async () => {
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$root:example.invalid"
    );
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const paginateBackwards = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      paginateBackwards
    });

    render(
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={transport}
        autoLoadOlderMessages
        automaticBackfillEligible={false}
        onReply={vi.fn()}
      />
    );

    expect(paginateBackwards).not.toHaveBeenCalled();

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: threadKey,
          generation: 1,
          items: []
        }
      }
    });

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          PaginationStateChanged: {
            request_id: null,
            key: threadKey,
            direction: "Backward",
            state: "Paginating"
          }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          PaginationStateChanged: {
            request_id: null,
            key: threadKey,
            direction: "Backward",
            state: "Idle"
          }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          GapPositionsUpdated: {
            key: threadKey,
            actor_generation: 1,
            generation: 2,
            positions: []
          }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          GapRepairReleased: {
            key: threadKey,
            actor_generation: 1,
            generation: 3
          }
        }
      });
    });

    await act(async () => Promise.resolve());
    expect(paginateBackwards).not.toHaveBeenCalled();
    expect(screen.queryByTestId("timeline-spinner")).toBeNull();
  });

  it("keeps an old-root placeholder at latest activity and replaces it without canonical pagination", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const latestReply = {
      ...message("$old-root-latest:example.invalid", "standalone old-root reply"),
      timestamp_ms: 1_800_000_010_000,
      thread_root: "$old-root:example.invalid"
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: { request_id: null, key: KEY, generation: 1, items: [latestReply] }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          ThreadRootProjection: {
            key: KEY,
            projection: {
              root_event_id: "$old-root:example.invalid",
              activity_event_id: "$old-root-latest:example.invalid",
              activity_timestamp_ms: 1_800_000_010_000,
              state: { kind: "pending" }
            }
          }
        }
      });
    });

    const pending = await screen.findByRole("status");
    const pendingRow = pending.closest<HTMLElement>("article");
    expect(pending.textContent).toContain("Loading thread message");
    expect(pendingRow?.getAttribute("data-row-id")).toBe(
      "thread-root:$old-root:example.invalid"
    );
    expect(pendingRow?.getAttribute("data-content-event-id")).toBe("$old-root:example.invalid");
    expect(pendingRow?.getAttribute("data-activity-event-id")).toBe(
      "$old-root-latest:example.invalid"
    );
    expect(screen.queryByText("standalone old-root reply")).toBeNull();

    const loadedRoot = {
      ...message("$old-root:example.invalid", "hydrated original root"),
      timestamp_ms: 1_700_000_000_000,
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$old-root-latest:example.invalid",
        latest_sender: null,
        latest_sender_label: null,
        latest_body_preview: null,
        latest_timestamp_ms: 1_800_000_010_000
      }
    };
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          ThreadRootProjection: {
            key: KEY,
            projection: {
              root_event_id: "$old-root:example.invalid",
              activity_event_id: "$old-root-latest:example.invalid",
              activity_timestamp_ms: 1_800_000_010_000,
              state: { kind: "ready", item: loadedRoot }
            }
          }
        }
      });
    });

    const readyRow = await screen.findByText("hydrated original root").then((node) =>
      node.closest<HTMLElement>("article")
    );
    expect(readyRow?.getAttribute("data-row-id")).toBe(
      "thread-root:$old-root:example.invalid"
    );
    expect(readyRow?.getAttribute("data-activity-event-id")).toBe(
      "$old-root-latest:example.invalid"
    );
  });

  it("keeps a terminal old-root failure visible without restoring a reply row", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const latestReply = {
      ...message("$failed-root-latest:example.invalid", "reply must remain suppressed"),
      timestamp_ms: 1_800_000_020_000,
      thread_root: "$failed-root:example.invalid"
    };
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: { request_id: null, key: KEY, generation: 1, items: [latestReply] }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          ThreadRootProjection: {
            key: KEY,
            projection: {
              root_event_id: "$failed-root:example.invalid",
              activity_event_id: "$failed-root-latest:example.invalid",
              activity_timestamp_ms: 1_800_000_020_000,
              state: { kind: "failed", failure_kind: "notFound" }
            }
          }
        }
      });
    });

    const failed = await screen.findByRole("status");
    const failedRow = failed.closest<HTMLElement>("article");
    expect(failed.textContent).toContain("Thread message is unavailable");
    expect(failedRow?.getAttribute("data-thread-root-projection-state")).toBe("failed");
    expect(failedRow?.getAttribute("data-row-id")).toBe(
      "thread-root:$failed-root:example.invalid"
    );
    expect(screen.queryByText("reply must remain suppressed")).toBeNull();
  });

  it("keeps a Room root summary at its origin and suppresses canonical replies by default", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const paginateBackwards = vi.fn(async () => undefined);
    const rootTimestampMs = 1_800_000_000_000;
    const latestReplyTimestampMs = rootTimestampMs + 60_000;
    const root = {
      ...message("$default-thread-root:example.invalid", "Default root body"),
      timestamp_ms: rootTimestampMs,
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$default-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Default latest reply preview",
        latest_timestamp_ms: latestReplyTimestampMs
      }
    };
    const latestReply = {
      ...message("$default-thread-reply:example.invalid", "Default standalone reply"),
      timestamp_ms: latestReplyTimestampMs,
      thread_root: "$default-thread-root:example.invalid"
    };
    const transport = baseTransport({
      paginateBackwards,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView timelineKey={KEY} roomId="!room:example.invalid" transport={transport} onReply={vi.fn()} />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [root, message("$default-between:example.invalid", "Default between"), latestReply]
          }
        }
      });
    });

    const rootRow = await screen.findByText("Default root body").then((node) =>
      node.closest<HTMLElement>("article")
    );
    expect(rootRow?.getAttribute("data-row-id")).toBe(
      "thread-root:$default-thread-root:example.invalid"
    );
    expect(rootRow?.getAttribute("data-content-event-id")).toBe("$default-thread-root:example.invalid");
    expect(rootRow?.getAttribute("data-activity-event-id")).toBe("$default-thread-root:example.invalid");
    const latestReplyTime = new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(
      new Date(latestReplyTimestampMs)
    );
    expect(rootRow?.textContent).toContain(
      `1 reply · Bob: Default latest reply preview · ${latestReplyTime}`
    );
    expect(screen.queryByText("Default standalone reply")).toBeNull();
    expect(
      Array.from(document.querySelectorAll("article[data-row-id]")).map((row) =>
        row.getAttribute("data-content-event-id")
      )
    ).toEqual(["$default-thread-root:example.invalid", "$default-between:example.invalid"]);
    expect(paginateBackwards).not.toHaveBeenCalled();
  });

  it("keeps the root but hides conversation-start chrome and its summary in thread presentation", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$thread-root:example.invalid"
    );
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const root = {
      ...message("$thread-root:example.invalid", "Thread root remains visible"),
      thread_summary: {
        reply_count: 2,
        latest_event_id: "$thread-latest:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "latest reply",
        latest_timestamp_ms: 1_800_000_010_000
      }
    };

    render(
      <TimelineView
        presentationContext="thread"
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: { request_id: null, key: threadKey, generation: 1, items: [root] }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          PaginationStateChanged: {
            request_id: null,
            key: threadKey,
            direction: "Backward",
            state: "EndReached"
          }
        }
      });
    });

    expect(await screen.findByText("Thread root remains visible")).not.toBeNull();
    expect(screen.queryByText("Start of conversation")).toBeNull();
    expect(screen.queryByRole("button", { name: /2 replies/i })).toBeNull();
  });

  it("moves one Room thread root and its summary to its latest reply while keeping root actions and timestamps", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onOpenThread = vi.fn();
    const onOpenContextMenu = vi.fn();
    const viewportObservations: Array<{
      roomId: string;
      firstVisibleEventId: string | null;
      lastVisibleEventId: string | null;
    }> = [];
    const observeViewport = vi.fn(
      async (
        roomId: string,
        firstVisibleEventId: string | null,
        lastVisibleEventId: string | null,
        _visibleGapIds: TimelineGapId[],
        _atBottom: boolean
      ) => {
        viewportObservations.push({ roomId, firstVisibleEventId, lastVisibleEventId });
      }
    );
    const rootTimestampMs = 1_800_000_000_000;
    const replyTimestampMs = rootTimestampMs + 60 * 60 * 1_000;
    const root = {
      ...message("$thread-root:example.invalid", "Original root body"),
      timestamp_ms: rootTimestampMs,
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply preview",
        latest_timestamp_ms: replyTimestampMs
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply body"),
      timestamp_ms: replyTimestampMs,
      thread_root: "$thread-root:example.invalid"
    };
    const rects = {
      "$before:example.invalid": { top: -100, height: 20 },
      "$between:example.invalid": { top: -100, height: 20 },
      "$latest-thread-reply:example.invalid": { top: 20, height: 40 },
      "$after:example.invalid": { top: 700, height: 20 }
    };
    const rectMock = mockTimelineRects(rects, { top: 0, height: 600 });
    const transport = baseTransport({
      observeViewport,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onOpenThread={onOpenThread}
        onOpenContextMenu={onOpenContextMenu}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$between:example.invalid", "Between"),
              latestReply,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    const rootRow = await screen.findByText("Original root body").then((node) =>
      node.closest<HTMLElement>("article")
    );
    expect(rootRow).not.toBeNull();
    expect(rootRow?.getAttribute("data-row-id")).toBe(
      "thread-root:$thread-root:example.invalid"
    );
    expect(rootRow?.getAttribute("data-content-event-id")).toBe("$thread-root:example.invalid");
    expect(rootRow?.getAttribute("data-activity-event-id")).toBe(
      "$latest-thread-reply:example.invalid"
    );
    expect(rootRow?.getAttribute("data-event-id")).toBe("$latest-thread-reply:example.invalid");
    expect(rootRow?.textContent).toContain(
      new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(new Date(rootTimestampMs))
    );
    expect(rootRow?.textContent).toContain("1 reply · Bob: Latest reply preview");
    expect(screen.queryByText("Standalone reply body")).toBeNull();
    expect(
      Array.from(document.querySelectorAll("article[data-row-id]")).map((row) =>
        row.getAttribute("data-content-event-id")
      )
    ).toEqual([
      "$before:example.invalid",
      "$between:example.invalid",
      "$thread-root:example.invalid",
      "$after:example.invalid"
    ]);

    fireEvent.click(screen.getByRole("button", { name: /Open thread, 1 reply/ }));
    expect(onOpenThread).toHaveBeenCalledWith(
      "!room:example.invalid",
      "$thread-root:example.invalid",
      "existingThread"
    );
    fireEvent.contextMenu(rootRow!);
    expect(onOpenContextMenu).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        kind: "message",
        message: expect.objectContaining({ event_id: "$thread-root:example.invalid" })
      }),
      expect.any(Array)
    );
    await waitFor(() => {
      expect(
        viewportObservations.some(
          ({ roomId, firstVisibleEventId, lastVisibleEventId }) =>
            roomId === "!room:example.invalid" &&
            firstVisibleEventId === "$latest-thread-reply:example.invalid" &&
            lastVisibleEventId === "$latest-thread-reply:example.invalid"
        )
      ).toBe(true);
    });
    rectMock.mockRestore();
  });

  it("keeps a replay-summary root out of the free-scroll anchor while using its activity identity", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onScrollDiagnosticsChange = vi.fn();
    const viewportObservations: Array<{
      firstVisibleEventId: string | null;
      lastVisibleEventId: string | null;
    }> = [];
    const observeViewport = vi.fn(
      async (
        _roomId: string,
        firstVisibleEventId: string | null,
        lastVisibleEventId: string | null,
        _visibleGapIds: TimelineGapId[],
        _atBottom: boolean
      ) => {
        viewportObservations.push({ firstVisibleEventId, lastVisibleEventId });
      }
    );
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectMock = mockPresentationOrderRects(scrollContainerRef);
    const rootEventId = "$replay-summary-root:example.invalid";
    const firstActivityEventId = "$summary-activity-first:example.invalid";
    const laterActivityEventId = "$summary-activity-later:example.invalid";
    const rootTimestampMs = 1_800_000_000_000;
    const firstActivityTimestampMs = rootTimestampMs + 2_000;
    const laterActivityTimestampMs = rootTimestampMs + 4_000;
    const root = {
      ...message(rootEventId, "Replay summary root"),
      timestamp_ms: rootTimestampMs,
      thread_summary: {
        reply_count: 1,
        latest_event_id: firstActivityEventId,
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Summary-only activity",
        latest_timestamp_ms: firstActivityTimestampMs
      }
    };
    const rootWithLaterSummary = {
      ...root,
      thread_summary: {
        ...root.thread_summary,
        latest_event_id: laterActivityEventId,
        latest_timestamp_ms: laterActivityTimestampMs
      }
    };
    const before = {
      ...message("$before-summary-root:example.invalid", "Before"),
      timestamp_ms: rootTimestampMs + 1_000
    };
    const after = {
      ...message("$after-summary-root:example.invalid", "After"),
      timestamp_ms: rootTimestampMs + 3_000
    };
    const transport = baseTransport({
      observeViewport,
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = () => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onScrollDiagnosticsChange={onScrollDiagnosticsChange}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );
    const { rerender } = render(renderView());

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          NavigationUpdated: {
            key: KEY,
            snapshot: navigationSnapshot({
              first_unread_event_id: firstActivityEventId,
              unread_event_count: 1,
              unread_position: "insideViewport"
            })
          }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          InitialItems: { request_id: null, key: KEY, generation: 1, items: [before, after] }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          ThreadRootProjection: {
            key: KEY,
            projection: {
              root_event_id: rootEventId,
              activity_event_id: firstActivityEventId,
              activity_timestamp_ms: firstActivityTimestampMs,
              retain_without_reply: true,
              source: { kind: "replayKnown", epoch: 1 },
              state: { kind: "ready", item: root }
            }
          }
        }
      });
    });

    const rootRow = await screen.findByText("Replay summary root").then((node) =>
      node.closest<HTMLElement>("article")
    );
    expect(rootRow?.getAttribute("data-content-event-id")).toBe(rootEventId);
    expect(rootRow?.getAttribute("data-activity-event-id")).toBe(firstActivityEventId);
    expect(
      Array.from(document.querySelectorAll("article[data-row-id]")).map((row) =>
        row.getAttribute("data-row-id")
      )
    ).toEqual([
      "$before-summary-root:example.invalid",
      `thread-root:${rootEventId}`,
      "$after-summary-root:example.invalid"
    ]);
    const unreadMarker = await screen.findByRole("separator", { name: "Unread messages" });
    expect(unreadMarker.nextElementSibling).toBe(rootRow);
    await waitFor(() => {
      expect(
        viewportObservations.some(
          ({ firstVisibleEventId, lastVisibleEventId }) =>
            firstVisibleEventId === "$before-summary-root:example.invalid" &&
            lastVisibleEventId === firstActivityEventId
        )
      ).toBe(true);
    });

    const timeline = screen.getByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      rerender(renderView());
    });
    await waitFor(() => expect(timeline.scrollTop).toBe(800));
    timeline.scrollTop = 190;
    fireEvent.wheel(timeline, { deltaY: -1 });
    fireEvent.scroll(timeline);

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          ThreadRootProjection: {
            key: KEY,
            projection: {
              root_event_id: rootEventId,
              activity_event_id: laterActivityEventId,
              activity_timestamp_ms: laterActivityTimestampMs,
              retain_without_reply: true,
              source: { kind: "replayKnown", epoch: 2 },
              state: { kind: "ready", item: rootWithLaterSummary }
            }
          }
        }
      });
    });

    await waitFor(() => {
      // The unchanged normal row stays at the same pixel. If the movable
      // summary root were used as the anchor, this would instead become 290.
      expect(timeline.scrollTop).toBe(90);
      expect(screen.getByText("After").closest("article")?.getBoundingClientRect().top).toBe(10);
      expect(
        onScrollDiagnosticsChange.mock.calls.some(
          ([diagnostics]) => diagnostics.scrollWrites.projectionCompensation > 0
        )
      ).toBe(true);
      expect(
        viewportObservations.some(
          ({ lastVisibleEventId }) => lastVisibleEventId === laterActivityEventId
        )
      ).toBe(true);
    });
    expect(rootRow?.getAttribute("data-activity-event-id")).toBe(laterActivityEventId);
    rectMock.mockRestore();
  });

  it("uses a non-moving row, never the moved root, when latest-reply placement toggles in free scroll", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onScrollDiagnosticsChange = vi.fn();
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectMock = mockPresentationOrderRects(scrollContainerRef);
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = (threadRootOrder: "rootEvent" | "latestReply") => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onScrollDiagnosticsChange={onScrollDiagnosticsChange}
        threadRootOrder={{ kind: threadRootOrder }}
      />
    );
    const { rerender } = render(renderView("rootEvent"));

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$between:example.invalid", "Between"),
              latestReply,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    await screen.findByText("Between");
    const timeline = screen.getByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 190,
      writable: true,
      configurable: true
    });
    // Let first-entry live-edge initialization finish before the test gives
    // the viewport back to a user-controlled free-scroll position.
    act(() => {
      rerender(renderView("rootEvent"));
    });
    await waitFor(() => {
      expect(timeline.scrollTop).toBe(800);
    });
    timeline.scrollTop = 190;
    fireEvent.wheel(timeline, { deltaY: -1 });
    fireEvent.scroll(timeline);

    act(() => {
      rerender(renderView("latestReply"));
    });

    await waitFor(() => {
      expect(timeline.scrollTop).toBe(90);
      expect(
        onScrollDiagnosticsChange.mock.calls.some(
          ([diagnostics]) => diagnostics.scrollWrites.projectionCompensation > 0
        )
      ).toBe(true);
    });
    expect(screen.getByText("Between").closest("article")?.getBoundingClientRect().top).toBe(10);
    rectMock.mockRestore();
  });

  it("keeps a committed projection compensation when StrictMode abandons a later render", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    let controls: {
      setOrder: (order: "rootEvent" | "latestReply") => void;
      setShouldSuspend: (shouldSuspend: boolean) => void;
      refresh: () => void;
    } | null = null;
    const suspended = new Promise<never>(() => undefined);
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectMock = mockPresentationOrderRects(scrollContainerRef);
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    function SuspendsAfterTimeline({ shouldSuspend }: { shouldSuspend: boolean }) {
      if (shouldSuspend) {
        throw suspended;
      }
      return null;
    }
    function Harness() {
      const [order, setOrder] = useState<"rootEvent" | "latestReply">("rootEvent");
      const [shouldSuspend, setShouldSuspend] = useState(false);
      const [, setVersion] = useState(0);
      useEffect(() => {
        controls = {
          setOrder,
          setShouldSuspend,
          refresh: () => setVersion((current) => current + 1)
        };
      });
      return (
        <Suspense fallback={null}>
          <TimelineView
            timelineKey={KEY}
            roomId="!room:example.invalid"
            transport={transport}
            onReply={vi.fn()}
            threadRootOrder={{ kind: order }}
          />
          <SuspendsAfterTimeline shouldSuspend={shouldSuspend} />
        </Suspense>
      );
    }

    render(
      <StrictMode>
        <Harness />
      </StrictMode>
    );
    await waitFor(() => expect(controls).not.toBeNull());
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$between:example.invalid", "Between"),
              latestReply,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    const timeline = await screen.findByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      controls!.refresh();
    });
    await waitFor(() => expect(timeline.scrollTop).toBe(800));
    timeline.scrollTop = 190;
    fireEvent.wheel(timeline, { deltaY: -1 });

    vi.useFakeTimers();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrameId = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      nextFrameId += 1;
      frames.set(nextFrameId, callback);
      return nextFrameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frameId) => {
      frames.delete(frameId);
    });

    // B commits and queues its free-scroll correction. C starts afterwards,
    // but suspends before it can commit; B remains the visible projection.
    act(() => {
      controls!.setOrder("latestReply");
    });
    expect(
      document
        .querySelector('[data-content-event-id="$thread-root:example.invalid"]')
        ?.getAttribute("data-activity-event-id")
    ).toBe("$latest-thread-reply:example.invalid");
    act(() => {
      startTransition(() => {
        controls!.setOrder("rootEvent");
        controls!.setShouldSuspend(true);
      });
    });
    expect(
      document
        .querySelector('[data-content-event-id="$thread-root:example.invalid"]')
        ?.getAttribute("data-activity-event-id")
    ).toBe("$latest-thread-reply:example.invalid");

    act(() => {
      const queued = [...frames.values()];
      frames.clear();
      for (const callback of queued) {
        callback(0);
      }
    });

    expect(timeline.scrollTop).toBe(90);
    rectMock.mockRestore();
  });

  it("does not overwrite a user scroll that happens after projection compensation is queued", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onScrollDiagnosticsChange = vi.fn();
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectMock = mockPresentationOrderRects(scrollContainerRef);
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = (threadRootOrder: "rootEvent" | "latestReply") => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onScrollDiagnosticsChange={onScrollDiagnosticsChange}
        threadRootOrder={{ kind: threadRootOrder }}
      />
    );
    const { rerender } = render(renderView("rootEvent"));

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$between:example.invalid", "Between"),
              latestReply,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    await screen.findByText("Between");
    const timeline = screen.getByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      rerender(renderView("rootEvent"));
    });
    await waitFor(() => expect(timeline.scrollTop).toBe(800));
    timeline.scrollTop = 190;
    fireEvent.wheel(timeline, { deltaY: -1 });

    vi.useFakeTimers();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrameId = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      nextFrameId += 1;
      frames.set(nextFrameId, callback);
      return nextFrameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frameId) => {
      frames.delete(frameId);
    });
    act(() => {
      rerender(renderView("latestReply"));
    });

    // A real user scroll takes ownership while the projection's frame is held.
    timeline.scrollTop = 250;
    fireEvent.wheel(timeline, { deltaY: -1 });
    fireEvent.scroll(timeline);
    act(() => {
      const queued = [...frames.values()];
      frames.clear();
      for (const callback of queued) {
        callback(0);
      }
    });

    expect(timeline.scrollTop).toBe(250);
    expect(
      onScrollDiagnosticsChange.mock.calls.some(
        ([diagnostics]) => diagnostics.scrollWrites.projectionCompensation > 0
      )
    ).toBe(false);
    rectMock.mockRestore();
  });

  it("does not apply queued projection compensation after a jump takes viewport ownership", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    let jumpToLatest: (() => void) | null = null;
    const onScrollDiagnosticsChange = vi.fn();
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    const rectMock = mockPresentationOrderRects(scrollContainerRef);
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = (threadRootOrder: "rootEvent" | "latestReply") => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onRegisterJumpToLatest={(handler) => {
          jumpToLatest = handler;
        }}
        onScrollDiagnosticsChange={onScrollDiagnosticsChange}
        threadRootOrder={{ kind: threadRootOrder }}
      />
    );
    const { rerender } = render(renderView("rootEvent"));

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$between:example.invalid", "Between"),
              latestReply,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    await screen.findByText("Between");
    const timeline = screen.getByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      rerender(renderView("rootEvent"));
    });
    await waitFor(() => expect(timeline.scrollTop).toBe(800));
    timeline.scrollTop = 190;
    fireEvent.wheel(timeline, { deltaY: -1 });

    vi.useFakeTimers();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrameId = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      nextFrameId += 1;
      frames.set(nextFrameId, callback);
      return nextFrameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frameId) => {
      frames.delete(frameId);
    });
    act(() => {
      rerender(renderView("latestReply"));
    });

    act(() => {
      jumpToLatest?.();
    });
    expect(timeline.scrollTop).toBe(800);
    act(() => {
      const queued = [...frames.values()];
      frames.clear();
      for (const callback of queued) {
        callback(0);
      }
    });

    expect(timeline.scrollTop).toBe(800);
    expect(
      onScrollDiagnosticsChange.mock.calls.some(
        ([diagnostics]) => diagnostics.scrollWrites.projectionCompensation > 0
      )
    ).toBe(false);
    rectMock.mockRestore();
  });

  it("renders an unread latest-reply marker before the root block that represents it", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          NavigationUpdated: {
            key: KEY,
            snapshot: navigationSnapshot({
              first_unread_event_id: "$latest-thread-reply:example.invalid",
              unread_event_count: 1,
              unread_position: "insideViewport"
            })
          }
        }
      });
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [message("$before:example.invalid", "Before"), root, latestReply]
          }
        }
      });
    });

    const marker = await screen.findByRole("separator", { name: "Unread messages" });
    const rootRow = marker.nextElementSibling;
    expect(rootRow?.getAttribute("data-content-event-id")).toBe("$thread-root:example.invalid");
    expect(rootRow?.getAttribute("data-activity-event-id")).toBe(
      "$latest-thread-reply:example.invalid"
    );
  });

  it("jumps to a moved root by its latest activity identity", async () => {
    const originalScrollIntoView = Element.prototype.scrollIntoView;
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      let emit: (payload: CoreEventPayload) => void = () => undefined;
      const root = {
        ...message("$thread-root:example.invalid", "Thread root"),
        thread_summary: {
          reply_count: 1,
          latest_event_id: "$latest-thread-reply:example.invalid",
          latest_sender: "@bob:example.invalid",
          latest_sender_label: "Bob",
          latest_body_preview: "Latest reply",
          latest_timestamp_ms: 1_800_000_001_000
        }
      };
      const latestReply = {
        ...message("$latest-thread-reply:example.invalid", "Standalone reply"),
        timestamp_ms: 1_800_000_001_000,
        thread_root: "$thread-root:example.invalid"
      };
      const transport = baseTransport({
        listenCoreEvents(nextListener) {
          emit = nextListener;
          return () => undefined;
        }
      });

      render(
        <TimelineView
          timelineKey={KEY}
          roomId="!room:example.invalid"
          transport={transport}
          onReply={vi.fn()}
          threadRootOrder={{ kind: "latestReply" }}
        />
      );

      act(() => {
        emit({
          kind: "Timeline",
          event: {
            NavigationUpdated: {
              key: KEY,
              snapshot: navigationSnapshot({
                first_unread_event_id: "$latest-thread-reply:example.invalid",
                unread_event_count: 1,
                unread_position: "belowViewport"
              })
            }
          }
        });
        emit({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key: KEY,
              generation: 1,
              items: [message("$before:example.invalid", "Before"), root, latestReply]
            }
          }
        });
      });

      fireEvent.click(await screen.findByRole("button", { name: /Jump to first unread/ }));
      expect(scrollIntoView).toHaveBeenCalledTimes(1);
      const jumpedRow = scrollIntoView.mock.instances[0] as HTMLElement | undefined;
      expect(jumpedRow?.getAttribute("data-content-event-id")).toBe(
        "$thread-root:example.invalid"
      );
    } finally {
      Element.prototype.scrollIntoView = originalScrollIntoView;
    }
  });

  it("keeps live edge pinned when a summary Set relocates its root block", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const oldRoot = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$older-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Older reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const newRoot = {
      ...oldRoot,
      thread_summary: {
        ...oldRoot.thread_summary,
        latest_event_id: "$newer-thread-reply:example.invalid",
        latest_body_preview: "Newer reply",
        latest_timestamp_ms: 1_800_000_003_000
      }
    };
    const olderReply = {
      ...message("$older-thread-reply:example.invalid", "Older reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const newerReply = {
      ...message("$newer-thread-reply:example.invalid", "Newer reply"),
      timestamp_ms: 1_800_000_003_000,
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = () => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );
    const { rerender } = render(renderView());

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              oldRoot,
              olderReply,
              message("$between:example.invalid", "Between"),
              newerReply
            ]
          }
        }
      });
    });

    await screen.findByText("Thread root");
    const timeline = screen.getByTestId("timeline-view");
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 1_200, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      rerender(renderView());
    });
    await waitFor(() => {
      expect(timeline.scrollTop).toBe(1_000);
    });

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          ItemsUpdated: {
            key: KEY,
            generation: 1,
            batch_id: 1,
            diffs: [{ Set: { index: 0, item: newRoot } }]
          }
        }
      });
    });

    await waitFor(() => {
      expect(timeline.scrollTop).toBe(1_000);
    });
  });

  it("falls back to the virtual height model when a projection anchor unmounts", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onScrollDiagnosticsChange = vi.fn();
    const rowHeight = 72;
    const normalCount = 620;
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$older-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "Older reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const updatedRoot = {
      ...root,
      thread_summary: {
        ...root.thread_summary,
        latest_event_id: "$newer-thread-reply:example.invalid",
        latest_body_preview: "Newer reply",
        latest_timestamp_ms: 1_800_000_003_000
      }
    };
    const olderReply = {
      ...message("$older-thread-reply:example.invalid", "Older reply"),
      timestamp_ms: 1_800_000_001_000,
      thread_root: "$thread-root:example.invalid"
    };
    const newerReply = {
      ...message("$newer-thread-reply:example.invalid", "Newer reply"),
      timestamp_ms: 1_800_000_003_000,
      thread_root: "$thread-root:example.invalid"
    };
    const normals = Array.from({ length: normalCount }, (_, index) =>
      message(`$normal${index}:example.invalid`, `Normal ${index}`)
    );
    const scrollContainerRef: { current: HTMLElement | null } = { current: null };
    let rootMovedToNewReply = false;
    const rectMock = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const timeline = scrollContainerRef.current;
        if (this.getAttribute("data-testid") === "timeline-view") {
          return {
            x: 0,
            y: 0,
            top: 0,
            left: 0,
            right: 0,
            width: 0,
            height: 200,
            bottom: 200,
            toJSON: () => ({})
          } as DOMRect;
        }
        const row = this.matches(".timeline-item-frame")
          ? this
          : this.closest<HTMLElement>(".timeline-item-frame");
        const rowId =
          row?.dataset["frameItemId"] ??
          row?.querySelector<HTMLElement>("[data-item-id]")?.dataset["itemId"] ??
          "";
        let rowIndex = -1;
        if (rowId.startsWith("date-divider:")) {
          rowIndex = 0;
        } else if (rowId === "thread-root:$thread-root:example.invalid") {
          rowIndex = rootMovedToNewReply ? normalCount + 1 : 1;
        } else {
          const match = /^\$normal(\d+):example\.invalid$/.exec(rowId);
          if (match) {
            rowIndex = Number(match[1]) + (rootMovedToNewReply ? 1 : 2);
          }
        }
        const top =
          rowIndex >= 0 ? rowIndex * rowHeight - (timeline?.scrollTop ?? 0) : 0;
        return {
          x: 0,
          y: top,
          top,
          left: 0,
          right: 0,
          width: 0,
          height: rowIndex >= 0 ? rowHeight : 0,
          bottom: top + (rowIndex >= 0 ? rowHeight : 0),
          toJSON: () => ({})
        } as DOMRect;
      });
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const renderView = () => (
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onScrollDiagnosticsChange={onScrollDiagnosticsChange}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );
    const { rerender } = render(renderView());

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [root, olderReply, ...normals, newerReply]
          }
        }
      });
    });

    await screen.findByText("Thread root");
    const timeline = screen.getByTestId("timeline-view");
    scrollContainerRef.current = timeline;
    Object.defineProperty(timeline, "clientHeight", { value: 200, configurable: true });
    Object.defineProperty(timeline, "scrollHeight", { value: 50_000, configurable: true });
    Object.defineProperty(timeline, "scrollTop", {
      value: 0,
      writable: true,
      configurable: true
    });
    act(() => {
      rerender(renderView());
    });
    expect(timeline.getAttribute("data-virtualized")).toBe("true");

    // The previous presentation puts Normal 300 after a date divider and the
    // root block. Its first-visible offset is +10px.
    timeline.scrollTop = 302 * rowHeight - 10;
    fireEvent.wheel(timeline, { deltaY: -1 });
    fireEvent.scroll(timeline);
    await waitFor(() => {
      expect(
        document.querySelector('[data-content-event-id="$normal300:example.invalid"]')
      ).not.toBeNull();
    });
    vi.useFakeTimers();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrameId = 0;
    let executedFrameCount = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      nextFrameId += 1;
      frames.set(nextFrameId, callback);
      return nextFrameId;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frameId) => {
      frames.delete(frameId);
    });
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          ItemsUpdated: {
            key: KEY,
            generation: 1,
            batch_id: 1,
            diffs: [{ Set: { index: 0, item: updatedRoot } }]
          }
        }
      });
    });
    rootMovedToNewReply = true;
    const transactionFrameScheduled = frames.size > 0;

    // Model a virtual-window turnover between commit and the coalesced frame:
    // the stable anchor is no longer mounted, so DOM restoration must fail and
    // the height-model offset is the only valid correction path.
    document
      .querySelector('[data-content-event-id="$normal300:example.invalid"]')
      ?.closest(".timeline-item-frame")
      ?.remove();
    act(() => {
      const queued = [...frames.values()];
      frames.clear();
      for (const callback of queued) {
        executedFrameCount += 1;
        callback(0);
      }
    });

    expect({
      transactionFrameScheduled,
      executedFrameCount: executedFrameCount > 0,
      projectionWriteRecorded: onScrollDiagnosticsChange.mock.calls.some(
        ([diagnostics]) => diagnostics.scrollWrites.projectionCompensation > 0
      ),
      scrollTop: timeline.scrollTop
    }).toEqual({
      transactionFrameScheduled: true,
      executedFrameCount: true,
      projectionWriteRecorded: true,
      scrollTop: 301 * rowHeight - 10
    });
    rectMock.mockRestore();
  });

  it("does not reorder Thread timeline rows when latest placement is enabled", async () => {
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$thread-root:example.invalid"
    );
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 1,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: null,
        latest_body_preview: "Latest reply",
        latest_timestamp_ms: 1_800_000_001_000
      }
    };
    const latestReply = {
      ...message("$latest-thread-reply:example.invalid", "Thread reply"),
      thread_root: "$thread-root:example.invalid"
    };
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });

    render(
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        threadRootOrder={{ kind: "latestReply" }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: threadKey,
            generation: 1,
            items: [root, latestReply]
          }
        }
      });
    });

    await screen.findByText("Thread reply");
    expect(
      Array.from(document.querySelectorAll("article[data-row-id]")).map((row) =>
        row.getAttribute("data-content-event-id")
      )
    ).toEqual(["$thread-root:example.invalid", "$latest-thread-reply:example.invalid"]);
  });

  it("shows new thread replies on the matching root row without moving timeline rows", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const onOpenThread = vi.fn();
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const root = {
      ...message("$thread-root:example.invalid", "Thread root"),
      thread_summary: {
        reply_count: 4,
        latest_event_id: "$latest-thread-reply:example.invalid",
        latest_sender: "@bob:example.invalid",
        latest_sender_label: "Bob",
        latest_body_preview: "latest reply",
        latest_timestamp_ms: 1_800_000_000_500
      }
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onOpenThread={onOpenThread}
        threadAttention={{
          rootEventId: "$thread-root:example.invalid",
          notificationCount: 2,
          highlightCount: 0,
          liveEventMarkerCount: 2
        }}
      />
    );

    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [
              message("$before:example.invalid", "Before"),
              root,
              message("$after:example.invalid", "After")
            ]
          }
        }
      });
    });

    const newReplies = await screen.findByRole("button", { name: /View new replies · 2/ });
    expect(newReplies.closest("[data-event-id]")?.getAttribute("data-event-id")).toBe(
      "$thread-root:example.invalid"
    );
    const eventOrder = Array.from(document.querySelectorAll("article[data-event-id]")).map(
      (row) => row.getAttribute("data-event-id")
    );
    expect(eventOrder).toEqual([
      "$before:example.invalid",
      "$thread-root:example.invalid",
      "$after:example.invalid"
    ]);

    fireEvent.click(newReplies);
    expect(onOpenThread).toHaveBeenCalledWith(
      "!room:example.invalid",
      "$thread-root:example.invalid",
      "existingThread"
    );
  });

  it("lets users request missing room keys from undecryptable events", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const requestRoomKey = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey
    });
    const encrypted = {
      ...message("$encrypted", "Unable to decrypt message"),
      unable_to_decrypt: {
        session_id: "session-1",
        reason: "missingRoomKey" as const,
        can_request_keys: true,
        recovery_stage: null,
        recovery_guidance: null
      }
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [encrypted]
        }
      }
    });

    const button = await screen.findByRole("button", { name: "Request keys and retry" });
    fireEvent.click(button);

    expect(requestRoomKey).toHaveBeenCalledWith(
      "!room:example.invalid",
      "$encrypted",
      "user",
      KEY
    );
  });

  it("renders Rust-owned automatic request state without dispatching automatic commands", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const requestRoomKey = vi.fn(async () => undefined);
    const threadKey = threadTimelineKey(
      "@alice:example.invalid",
      "!room:example.invalid",
      "$thread-root:example.invalid"
    );
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey
    });
    const encrypted = {
      ...message("$encrypted-thread-reply:example.invalid", "Unable to decrypt message"),
      thread_root: "$thread-root:example.invalid",
      unable_to_decrypt: {
        session_id: "session-1",
        reason: "missingRoomKey" as const,
        can_request_keys: true,
        recovery_stage: null,
        recovery_guidance: null
      },
      request_state: { stage: "automatic", withheldCode: null } as const
    };

    render(
      <TimelineView
        timelineKey={threadKey}
        roomId="!room:example.invalid"
        presentationContext="thread"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: threadKey,
          generation: 1,
          items: [encrypted]
        }
      }
    });

    // Automatic admission is Rust-owned: the frontend dispatches nothing and
    // only renders the Rust-published request state (awaiting copy).
    await waitFor(() => {
      expect(requestRoomKey).not.toHaveBeenCalled();
    });
    expect(screen.queryByText("Waiting for the decryption key…")).toBeTruthy();
  });

  it("does not classify room-key request failures in React", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const privateEventId = "$private-event:example.invalid";
    const privateBody = "secret message body";
    const rawError = [
      "raw SDK error",
      "/Users/member/private/store",
      "https://private.example.invalid/room",
      "access_token=private-token"
    ].join(" ");
    const requestRoomKey = vi.fn(async () => {
      throw new Error(rawError);
    });
    const onDiagnosticLogEntry = vi.fn();
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey
    });
    const encrypted = {
      ...message(privateEventId, privateBody),
      unable_to_decrypt: {
        session_id: "private-session-id",
        reason: "missingRoomKey" as const,
        can_request_keys: true,
        recovery_stage: null,
        recovery_guidance: null
      }
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [encrypted]
        }
      }
    });

    fireEvent.click(await screen.findByRole("button", { name: "Request keys and retry" }));

    await waitFor(() => expect(requestRoomKey).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    const diagnosticText = JSON.stringify(onDiagnosticLogEntry.mock.calls);
    expect(diagnosticText).not.toContain("operation=request_keys stage=failed kind=transport");

    for (const privateValue of [
      "!room:example.invalid",
      privateEventId,
      privateBody,
      "private-session-id",
      rawError,
      "/Users/member/private/store",
      "private.example.invalid",
      "private-token"
    ]) {
      expect(diagnosticText).not.toContain(privateValue);
    }
  });

  it("renders the read marker after the Rust-derived display anchor for own messages after the marker", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const ownMessage = (eventId: string): TimelineItem => ({
      ...message(eventId, "own"),
      sender: "@alice:example.invalid"
    });
    const other = message("$other:example.invalid", "hello");
    const own1 = ownMessage("$own1:example.invalid");
    const own2 = ownMessage("$own2:example.invalid");

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        NavigationUpdated: {
          key: KEY,
          snapshot: {
            read_marker_event_id: "$other:example.invalid",
            read_marker_display_event_id: "$own2:example.invalid",
            first_unread_event_id: null,
            unread_event_count: 0,
            unread_position: "none",
            newer_event_count: 0,
            can_jump_to_bottom: false
          }
        }
      }
    });
    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [other, own1, own2]
        }
      }
    });

    const marker = await screen.findByRole("separator", { name: "Read up to here" });
    expect(marker.previousElementSibling?.getAttribute("data-event-id")).toBe(
      "$own2:example.invalid"
    );
  });

  it("renders the read marker after the current user's latest own message when the marker starts on an own message", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const ownMessage = (eventId: string): TimelineItem => ({
      ...message(eventId, "own"),
      sender: "@alice:example.invalid"
    });
    const own1 = ownMessage("$own1:example.invalid");
    const own2 = ownMessage("$own2:example.invalid");

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        NavigationUpdated: {
          key: KEY,
          snapshot: {
            read_marker_event_id: "$own1:example.invalid",
            read_marker_display_event_id: "$own2:example.invalid",
            first_unread_event_id: null,
            unread_event_count: 0,
            unread_position: "none",
            newer_event_count: 0,
            can_jump_to_bottom: false
          }
        }
      }
    });
    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [own1, own2]
        }
      }
    });

    const marker = await screen.findByRole("separator", { name: "Read up to here" });
    expect(marker.previousElementSibling?.getAttribute("data-event-id")).toBe(
      "$own2:example.invalid"
    );
  });

  it("renders the unread marker before the first unread event", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const other = message("$other:example.invalid", "hello");
    const unread = message("$unread:example.invalid", "new message");
    const own1 = { ...message("$own1:example.invalid", "own"), sender: "@alice:example.invalid" };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        NavigationUpdated: {
          key: KEY,
          snapshot: {
            read_marker_event_id: "$other:example.invalid",
            read_marker_display_event_id: null,
            first_unread_event_id: "$unread:example.invalid",
            unread_event_count: 1,
            unread_position: "insideViewport",
            newer_event_count: 0,
            can_jump_to_bottom: false
          }
        }
      }
    });
    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [other, unread, own1]
        }
      }
    });

    const marker = await screen.findByRole("separator", { name: "Unread messages" });
    expect(marker.nextElementSibling?.getAttribute("data-event-id")).toBe(
      "$unread:example.invalid"
    );
  });

  it("renders link preview cards as clickable anchors", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const hideLinkPreview = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      hideLinkPreview
    });
    const item: TimelineItem = {
      ...message("$preview:example.invalid", "look at this"),
      link_previews: [
        {
          url: "https://example.com/article",
          title: "An article",
          state: "ready"
        }
      ]
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [item]
        }
      }
    });

    const card = await screen.findByRole("link", { name: /An article/ });
    expect(card.getAttribute("href")).toBe("https://example.com/article");
    fireEvent.click(card);
    await waitFor(() => {
      expect(openExternalHttpUrl).toHaveBeenCalledWith("https://example.com/article");
    });

    const hide = screen.getByRole("button", { name: "Hide preview" });
    fireEvent.click(hide);
    await waitFor(() => {
      expect(hideLinkPreview).toHaveBeenCalledWith("!room:example.invalid", "$preview:example.invalid");
    });
  });

  it("emits private-data-free diagnostics when viewport pending link previews load", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const loadLinkPreviews = vi.fn(async () => undefined);
    const onDiagnosticLogEntry = vi.fn();
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      loadLinkPreviews
    });
    const item: TimelineItem = {
      ...message("$pending-preview:example.invalid", "look at https://secret.example/article"),
      link_previews: [
        {
          url: "https://secret.example/article",
          state: "pending"
        }
      ]
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        onDiagnosticLogEntry={onDiagnosticLogEntry}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [item]
        }
      }
    });

    await waitFor(() => {
      expect(loadLinkPreviews).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$pending-preview:example.invalid"
      );
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.preview",
          message: "kind=room stage=request trigger=viewport_pending pending=1"
        })
      );
    });

    emit({
      kind: "Timeline",
      event: {
        ItemsUpdated: {
          key: KEY,
          generation: 1,
          batch_id: 1,
          diffs: [
            {
              Set: {
                index: 0,
                item: {
                  ...item,
                  link_previews: [
                    {
                      url: "https://secret.example/article",
                      title: "Loaded",
                      state: "ready"
                    }
                  ]
                }
              }
            }
          ]
        }
      }
    });

    await waitFor(() => {
      expect(onDiagnosticLogEntry).toHaveBeenCalledWith(
        expect.objectContaining({
          source: "timeline.preview",
          message: "kind=room stage=update items=1 pending=0 loading=0 ready=1 failed=0"
        })
      );
    });

    const diagnosticText = onDiagnosticLogEntry.mock.calls
      .map(([entry]) => `${entry.source} ${entry.message}`)
      .join("\n");
    expect(diagnosticText).not.toContain("$pending-preview");
    expect(diagnosticText).not.toContain("secret.example");
  });

  it("limits initial link preview requests to the current viewport window", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const loadLinkPreviews = vi.fn(async () => undefined);
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      },
      loadLinkPreviews
    });
    const items = Array.from({ length: 40 }, (_, index) => ({
      ...message(`$preview-window-${index}`, `Preview row ${index}`),
      link_previews: [
        {
          url: `https://example.invalid/preview-window-${index}`,
          state: "pending" as const
        }
      ]
    }));

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items
        }
      }
    });

    await waitFor(() => {
      expect(loadLinkPreviews).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$preview-window-0"
      );
    });
    expect(loadLinkPreviews).not.toHaveBeenCalledWith(
      "!room:example.invalid",
      "$preview-window-39"
    );
    expect(loadLinkPreviews.mock.calls.length).toBeLessThan(items.length);
  });

  it("keeps reactions and read receipts in one footer status row", async () => {
    let emit: (payload: CoreEventPayload) => void = () => undefined;
    const transport = baseTransport({
      listenCoreEvents(nextListener) {
        emit = nextListener;
        return () => undefined;
      }
    });
    const item: TimelineItem = {
      ...message("$reacted:example.invalid", "hello"),
      reactions: [
        {
          key: "👍",
          count: 1,
          reacted_by_me: false,
          my_reaction_event_id: null,
          sender_preview: [{ user_id: "@bob:example.invalid", display_label: "Bob" }]
        }
      ],
      can_react: true
    };
    const liveSignals: LiveSignalsState = {
      rooms: {
        "!room:example.invalid": {
          receipts_by_event: {
            "$reacted:example.invalid": {
              readers: [
                {
                  user_id: "@bob:example.invalid",
                  display_name: "Bob",
                  original_display_label: "Bob",
                  avatar: null,
                  timestamp_ms: 1_800_000_000_000
                }
              ],
              total_count: 1,
              overflow_count: 0
            }
          },
          fully_read_event_id: null,
          typing_user_ids: [],
          typing_users: []
        }
      },
      presence: {}
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        liveSignals={liveSignals}
      />
    );

    emit({
      kind: "Timeline",
      event: {
        InitialItems: {
          request_id: null,
          key: KEY,
          generation: 1,
          items: [item]
        }
      }
    });

    const statusRow = await waitFor(() => {
      const row = document.querySelector(".message-status-row");
      if (!row) {
        throw new Error("message-status-row not found");
      }
      return row;
    });
    expect(statusRow.querySelector(".message-reactions")).toBeTruthy();
    expect(statusRow.querySelector(".message-receipts")).toBeTruthy();
  });

  it("renders typing indicators with room display labels when available", async () => {
    const transport = baseTransport({});
    const liveSignals: LiveSignalsState = {
      rooms: {
        "!room:example.invalid": {
          receipts_by_event: {},
          fully_read_event_id: null,
          typing_user_ids: ["@hironeishida:matrix.org"],
          typing_users: [
            {
              user_id: "@hironeishida:matrix.org",
              display_label: "Hirone Ishida"
            }
          ]
        }
      },
      presence: {}
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        liveSignals={liveSignals}
        profileUsers={{
          "@hironeishida:matrix.org": {
            user_id: "@hironeishida:matrix.org",
            display_name: "Hirone Ishida",
            display_label: "Hirone Ishida",
            original_display_label: "Hirone Ishida",
            mention_search_terms: [],
            avatar: null
          }
        }}
      />
    );

    expect(screen.getByText("Hirone Ishida is typing")).toBeTruthy();
    expect(screen.queryByText("@hironeishida:matrix.org is typing")).toBeNull();
  });

  it("uses a friendly fallback for typing indicators without a projected label", async () => {
    const transport = baseTransport({});
    const liveSignals: LiveSignalsState = {
      rooms: {
        "!room:example.invalid": {
          receipts_by_event: {},
          fully_read_event_id: null,
          typing_user_ids: ["@unknown:example.invalid"],
          typing_users: [
            {
              user_id: "@unknown:example.invalid",
              display_label: null
            }
          ]
        }
      },
      presence: {}
    };

    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
        liveSignals={liveSignals}
      />
    );

    expect(screen.getByText("Unknown user is typing")).toBeTruthy();
    expect(screen.queryByText("@unknown:example.invalid is typing")).toBeNull();
  });
});

describe("room key request feedback (#460)", () => {
  function utdItem(eventId: string, requestState: RoomKeyRequestStateDto | null) {
    return {
      ...message(eventId, "Unable to decrypt message"),
      unable_to_decrypt: {
        session_id: "session-1",
        reason: "missingRoomKey" as const,
        can_request_keys: true,
        recovery_stage: null,
        recovery_guidance: null
      },
      request_state: requestState
    };
  }

  function renderWithItems(items: TimelineItem[]) {
    let emit: (payload: unknown) => void = () => undefined;
    const transport = {
      listenCoreEvents(nextListener: (p: unknown) => void) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey: vi.fn(async () => undefined),
      ensureSubscribed: vi.fn(async () => undefined)
    } as never;
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items
          }
        }
      });
    });
    return { transport };
  }

  it("renders localized copy for each closed withheld code", () => {
    const cases: Array<[RoomKeyRequestWithheldCode | null, RegExp]> = [
      ["unavailable", /The requested device does not have this decryption key/],
      ["unauthorised", /Sharing the decryption key was not permitted/],
      ["unverified", /This device is unverified, so the key was not shared/],
      ["blacklisted", /This device is excluded from key sharing/],
      [null, /The decryption key could not be obtained/]
    ];
    for (const [code, expected] of cases) {
      renderWithItems([utdItem("$w", { stage: "withheld", withheldCode: code })]);
      expect(screen.queryByText(expected)).toBeTruthy();
      cleanup();
    }
  });

  it("still_waiting shows non-terminal guidance and never a raw reason", () => {
    renderWithItems([utdItem("$s", { stage: "still_waiting", withheldCode: null })]);
    expect(
      screen.queryByText(/No response yet. Another device may be offline/)
    ).toBeTruthy();
    expect(screen.queryByText(/m\.unauthorised|refused|denied/i)).toBeNull();
  });

  it("send_failed shows the generic refusal copy instead of nothing", () => {
    renderWithItems([utdItem("$f", { stage: "send_failed", withheldCode: null })]);
    expect(
      screen.queryByText("The decryption key could not be obtained.")
    ).toBeTruthy();
    expect(screen.queryByText(/Waiting for the decryption key/)).toBeNull();
  });

  it("decryption_recovered shows success and clears the pending marker", () => {
    renderWithItems([utdItem("$r", { stage: "decryption_recovered", withheldCode: null })]);
    expect(screen.queryByText("Decryption key received")).toBeTruthy();
    expect(screen.queryByText(/Waiting for the decryption key/)).toBeNull();
  });

  it("clicking Request keys shows an immediate toast and pending copy; repeat clicks are suppressed while pending", async () => {
    let emit: (payload: unknown) => void = () => undefined;
    const requestRoomKey = vi.fn(async () => undefined);
    const transport = {
      listenCoreEvents(nextListener: (p: unknown) => void) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey,
      ensureSubscribed: vi.fn(async () => undefined)
    } as never;
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [utdItem("$click", null)]
          }
        }
      });
    });
    const button = await screen.findByRole("button", { name: "Request keys and retry" });
    // Click twice: the toast/pending marker must not duplicate, and a repeat
    // click while the request is pending dispatches no duplicate command
    // (plan: no duplicate commands while pending; Rust also coalesces).
    fireEvent.click(button);
    fireEvent.click(button);
    await waitFor(() => {
      expect(document.body.textContent).toContain("Decryption key requested");
    });
    await waitFor(() => {
      expect(document.body.textContent).toContain("Waiting for the decryption key");
    });
    expect(screen.getAllByText(/Decryption key requested/)).toHaveLength(1);
    expect(screen.getAllByText(/Waiting for the decryption key/)).toHaveLength(1);
    expect(requestRoomKey).toHaveBeenCalledTimes(1);
    expect(requestRoomKey).toHaveBeenCalledWith(
      "!room:example.invalid",
      "$click",
      "user",
      KEY
    );
    // Suppression persists through the Rust-published pending (sent) stage:
    // a further click re-shows the toast but dispatches no new command.
    act(() => {
      emit({
        kind: "Room",
        event: {
          RoomKeyRequestStateChanged: {
            key: {
              account_key: "@alice:example.invalid",
              kind: { Room: { room_id: "!room:example.invalid" } }
            },
            event_id: "$click",
            request_id: null,
            stage: "sent",
            withheld_code: null
          }
        }
      });
    });
    await waitFor(() => {
      expect(document.body.textContent).toContain("Waiting for the decryption key");
    });
    fireEvent.click(button);
    expect(requestRoomKey).toHaveBeenCalledTimes(1);
  });

  it("keyboard activation requests keys and announces the toast in an ARIA-live status region", async () => {
    let emit: (payload: unknown) => void = () => undefined;
    const requestRoomKey = vi.fn(async () => undefined);
    const transport = {
      listenCoreEvents(nextListener: (p: unknown) => void) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey,
      ensureSubscribed: vi.fn(async () => undefined)
    } as never;
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [utdItem("$kb", null)]
          }
        }
      });
    });
    const button = await screen.findByRole("button", { name: "Request keys and retry" });
    // The action is a native <button> (browser-activated by Enter/Space);
    // jsdom does not synthesize the Enter->click translation, so activate it
    // while focused and assert the IPC payload + ARIA-live announcement.
    expect(button.tagName).toBe("BUTTON");
    button.focus();
    expect(document.activeElement).toBe(button);
    fireEvent.click(button);
    await waitFor(() => {
      expect(requestRoomKey).toHaveBeenCalledWith(
        "!room:example.invalid",
        "$kb",
        "user",
        KEY
      );
    });
    const status = screen.getByRole("status");
    expect(status.getAttribute("aria-live")).toBe("polite");
    expect(status.textContent).toContain("Decryption key requested");
  });

  it("a Rust-published transition clears the local pending marker and shows the terminal copy", async () => {
    let emit: (payload: unknown) => void = () => undefined;
    const transport = {
      listenCoreEvents(nextListener: (p: unknown) => void) {
        emit = nextListener;
        return () => undefined;
      },
      requestRoomKey: vi.fn(async () => undefined),
      ensureSubscribed: vi.fn(async () => undefined)
    } as never;
    render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    act(() => {
      emit({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: KEY,
            generation: 1,
            items: [utdItem("$click", null)]
          }
        }
      });
    });
    const button = await screen.findByRole("button", { name: "Request keys and retry" });
    fireEvent.click(button);
    await waitFor(() => {
      expect(document.body.textContent).toContain("Waiting for the decryption key");
    });
    // Rust settles the request as refused (withheld) via the typed event.
    act(() => {
      emit({
        kind: "Room",
        event: {
          RoomKeyRequestStateChanged: {
            key: {
              account_key: "@alice:example.invalid",
              kind: { Room: { room_id: "!room:example.invalid" } }
            },
            event_id: "$click",
            request_id: null,
            stage: "withheld",
            withheld_code: "unavailable"
          }
        }
      });
    });
    await waitFor(() => {
      expect(
        screen.queryByText(/The requested device does not have this decryption key/)
      ).toBeTruthy();
    });
    // The local pending marker is gone once the terminal state is rendered.
    expect(screen.queryByText(/Waiting for the decryption key/)).toBeNull();
  });

  it("a delayed rejection from an earlier visit does not clear the current pending marker (A->B->A)", async () => {
    let emitA: (payload: unknown) => void = () => undefined;
    let rejectFirst: (reason?: unknown) => void = () => undefined;
    let rejectSecond: (reason?: unknown) => void = () => undefined;
    const requestRoomKey = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            rejectFirst = reject;
          })
      )
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            rejectSecond = reject;
          })
      );
    const keyB = roomTimelineKey("@bob:example.invalid", "!room:example.invalid");
    const transport = {
      listenCoreEvents(nextListener: (p: unknown) => void) {
        emitA = nextListener;
        return () => undefined;
      },
      requestRoomKey,
      ensureSubscribed: vi.fn(async () => undefined)
    } as never;
    const { rerender } = render(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    const seed = (key: typeof KEY) =>
      act(() => {
        emitA({
          kind: "Timeline",
          event: {
            InitialItems: {
              request_id: null,
              key,
              generation: 1,
              items: [utdItem("$click", null)]
            }
          }
        });
      });
    seed(KEY);
    const button = await screen.findByRole("button", { name: "Request keys and retry" });
    fireEvent.click(button); // visit A, epoch 1
    await waitFor(() => {
      expect(document.body.textContent).toContain("Waiting for the decryption key");
    });
    // Navigate A -> B -> A (each switch bumps the view epoch).
    rerender(
      <TimelineView
        timelineKey={keyB}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    seed(keyB);
    rerender(
      <TimelineView
        timelineKey={KEY}
        roomId="!room:example.invalid"
        transport={transport}
        onReply={vi.fn()}
      />
    );
    seed(KEY);
    // New click in the final A visit (epoch 3) — new marker + request.
    const buttonAgain = await screen.findByRole("button", { name: "Request keys and retry" });
    fireEvent.click(buttonAgain);
    await waitFor(() => {
      expect(requestRoomKey).toHaveBeenCalledTimes(2);
    });
    // The FIRST visit's request rejects late: it must not clear the new marker.
    act(() => {
      rejectFirst(new Error("stale rejection"));
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText(/Waiting for the decryption key/)).toBeTruthy();
    // The CURRENT visit's own rejection still clears its marker.
    act(() => {
      rejectSecond(new Error("current rejection"));
    });
    await waitFor(() => {
      expect(screen.queryByText(/Waiting for the decryption key/)).toBeNull();
    });
  });
});
