import { useEffect, type RefObject } from "react";
import type { CoreEventPayload, TimelineKey } from "../../domain/coreEvents";
import type { TimelineTransport } from "./TimelineTransport";

type TimelineEventSubscriptionOptions = {
  transport: TimelineTransport;
  onEvent: (payload: CoreEventPayload) => void;
  itemCount: number;
  timelineKeyHash: string;
  timelineKeyHashRef: RefObject<string>;
  timelineKeyRef: RefObject<TimelineKey>;
  initialItemsSeenForTimelineKeyRef: RefObject<string | null>;
};

const TIMELINE_SUBSCRIBE_FALLBACK_DELAY_MS = 120;

export function useTimelineEventSubscription({
  transport,
  onEvent,
  itemCount,
  timelineKeyHash,
  timelineKeyHashRef,
  timelineKeyRef,
  initialItemsSeenForTimelineKeyRef
}: TimelineEventSubscriptionOptions): void {
  useEffect(() => transport.listenCoreEvents(onEvent), [onEvent, transport]);

  useEffect(() => {
    if (!transport.ensureSubscribed) {
      return;
    }
    if (itemCount > 0) {
      return;
    }
    const timelineKeyHashAtSchedule = timelineKeyHash;
    const timeoutId = window.setTimeout(() => {
      if (timelineKeyHashRef.current !== timelineKeyHashAtSchedule) {
        return;
      }
      if (initialItemsSeenForTimelineKeyRef.current === timelineKeyHashAtSchedule) {
        return;
      }
      void transport.ensureSubscribed?.(timelineKeyRef.current).catch(() => undefined);
    }, TIMELINE_SUBSCRIBE_FALLBACK_DELAY_MS);
    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [itemCount, timelineKeyHash, transport]);
}
