/**
 * Private-data-free counters for the event-driven timeline CoreEvent channel
 * (separate from the appStore StateDelta path), surfaced in the diagnostic
 * report. They disambiguate an empty timeline:
 *   - received=0                 -> Rust is not emitting Timeline events / the
 *                                   listener is not attached.
 *   - received>0, keyMismatch≈received -> events arrive but are dropped by the
 *                                   timelineKey equality filter (key mismatch).
 *   - initialItemsApplied>0, lastInitialItemsCount=0 -> events arrive and apply
 *                                   but carry no items (empty InitialItems).
 * Counts are cumulative for the session; no Matrix ids or bodies are recorded.
 */
export interface TimelineTransportStats {
  received: number;
  keyMismatchDropped: number;
  keyMismatchGroups: Record<string, number>;
  initialItemsApplied: number;
  lastInitialItemsCount: number;
  /**
   * Count of `ResyncMarker` events (a consumer lagged the core event broadcast
   * and the in-between events were dropped). A nonzero value confirms the
   * broadcast-overflow path is being hit; the timeline must re-subscribe to
   * recover the dropped `InitialItems`.
   */
  resync: number;
}

function zeroed(): TimelineTransportStats {
  return {
    received: 0,
    keyMismatchDropped: 0,
    keyMismatchGroups: {},
    initialItemsApplied: 0,
    lastInitialItemsCount: 0,
    resync: 0
  };
}

let stats: TimelineTransportStats = zeroed();
let lastMismatchSummaryMs: number | null = null;
const MISMATCH_SUMMARY_INTERVAL_MS = 30_000;

export function recordTimelineEventReceived(): void {
  stats.received += 1;
}

export function recordTimelineKeyMismatch(
  currentKind: string,
  eventKind: string,
  accountMatch: boolean,
  roomMatch: boolean,
  nowMs = Date.now()
): boolean {
  stats.keyMismatchDropped += 1;
  const group = `${currentKind}:${eventKind}:${accountMatch ? "account_match" : "account_mismatch"}:${roomMatch ? "room_match" : "room_mismatch"}`;
  stats.keyMismatchGroups[group] = (stats.keyMismatchGroups[group] ?? 0) + 1;
  if (
    lastMismatchSummaryMs !== null &&
    nowMs - lastMismatchSummaryMs < MISMATCH_SUMMARY_INTERVAL_MS
  ) {
    return false;
  }
  lastMismatchSummaryMs = nowMs;
  return true;
}

export function recordTimelineInitialItems(count: number): void {
  stats.initialItemsApplied += 1;
  stats.lastInitialItemsCount = Math.max(0, Math.trunc(count));
}

export function recordTimelineResync(): void {
  stats.resync += 1;
}

export function getTimelineTransportStats(): TimelineTransportStats {
  return { ...stats, keyMismatchGroups: { ...stats.keyMismatchGroups } };
}

export function resetTimelineTransportStats(): void {
  stats = zeroed();
  lastMismatchSummaryMs = null;
}
