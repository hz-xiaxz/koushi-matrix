import type { KeyboardEvent } from "react";
import type { TimelineGapId } from "../../domain/coreEvents";
import { eventIdForTimelineIdentity } from "./TimelineViewportAnchors";

const CANONICAL_UNSIGNED_DECIMAL = /^(?:0|[1-9][0-9]*)$/;
const MAX_U32 = 4_294_967_295;

function isCanonicalUnsignedDecimal(value: string | undefined): value is string {
  return value !== undefined && CANONICAL_UNSIGNED_DECIMAL.test(value);
}

function parseCanonicalU32(value: string | undefined): number | null {
  if (!isCanonicalUnsignedDecimal(value)) {
    return null;
  }
  let parsed = 0;
  for (const digit of value) {
    parsed = parsed * 10 + digit.charCodeAt(0) - "0".charCodeAt(0);
    if (parsed > MAX_U32) {
      return null;
    }
  }
  return parsed;
}

export function visibleTimelineViewportFacts(container: HTMLElement): {
  firstVisibleEventId: string | null;
  lastVisibleEventId: string | null;
  visibleGapIds: TimelineGapId[];
} {
  const containerRect = container.getBoundingClientRect();
  const nodes = container.querySelectorAll<HTMLElement>("[data-activity-event-id]");
  let firstVisibleEventId: string | null = null;
  let lastVisibleEventId: string | null = null;
  for (const node of nodes) {
    const rect = node.getBoundingClientRect();
    if (rect.bottom <= containerRect.top || rect.top >= containerRect.bottom) {
      continue;
    }
    const eventId = eventIdForTimelineIdentity(node, "activity");
    if (!eventId) {
      continue;
    }
    firstVisibleEventId ??= eventId;
    lastVisibleEventId = eventId;
  }
  const visibleGapIds: TimelineGapId[] = [];
  const seenGapIds = new Set<string>();
  const gapNodes = container.querySelectorAll<HTMLElement>(
    "[data-gap-topology-revision][data-gap-ordinal]"
  );
  for (const node of gapNodes) {
    const rect = node.getBoundingClientRect();
    if (rect.bottom <= containerRect.top || rect.top >= containerRect.bottom) {
      continue;
    }
    const topologyRevision = node.dataset["gapTopologyRevision"];
    const ordinal = parseCanonicalU32(node.dataset["gapOrdinal"]);
    if (!isCanonicalUnsignedDecimal(topologyRevision) || ordinal === null) {
      continue;
    }
    const signature = `${topologyRevision}\u0000${ordinal}`;
    if (seenGapIds.has(signature)) {
      continue;
    }
    seenGapIds.add(signature);
    visibleGapIds.push({ topology_revision: topologyRevision, ordinal });
  }
  return { firstVisibleEventId, lastVisibleEventId, visibleGapIds };
}

export function isScrolledToBottom(container: HTMLElement): boolean {
  return (
    container.scrollHeight - container.clientHeight - container.scrollTop <=
    SCROLL_EDGE_TOLERANCE_PX
  );
}

export function scrollContainerToBottom(container: HTMLElement): void {
  container.scrollTop = container.scrollHeight - container.clientHeight;
}

export function timelineKeyShouldReleaseViewportIntent(event: KeyboardEvent<HTMLDivElement>): boolean {
  if (event.altKey || event.ctrlKey || event.metaKey) {
    return false;
  }
  switch (event.key) {
    case "ArrowDown":
    case "ArrowUp":
    case "End":
    case "Home":
    case "PageDown":
    case "PageUp":
    case " ":
      return true;
    default:
      return false;
  }
}

/** Distance (px) from the top edge that triggers automatic backfill. */
const AUTO_BACKFILL_THRESHOLD_PX = 80;
const AUTO_BACKFILL_VIEWPORTS = 2;
export const SCROLL_EDGE_TOLERANCE_PX = 2;

export function timelineBackfillThreshold(clientHeight: number, enabled: boolean): number {
  if (!enabled) {
    return 0;
  }
  return Math.max(
    AUTO_BACKFILL_THRESHOLD_PX,
    Math.max(0, clientHeight) * AUTO_BACKFILL_VIEWPORTS
  );
}

export const timelineBackfillThresholdForTests = timelineBackfillThreshold;
