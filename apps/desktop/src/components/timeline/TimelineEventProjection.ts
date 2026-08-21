import type {
  TimelineAnchorRestoreStatus,
  TimelineDiff,
  TimelineEvent,
  TimelineItem,
  TimelineKey,
  PaginationState
} from "../../domain/coreEvents";
import type { getPaginationState } from "../../domain/timelineStore";

export function timelineDiffsContainOwnOutgoingItem(
  diffs: readonly TimelineDiff[],
  currentUserId: string | undefined
): boolean {
  if (!currentUserId) {
    return false;
  }
  return diffs.some((diff) => timelineDiffItems(diff).some((item) => timelineItemIsOwnOutgoing(item, currentUserId)));
}

function timelineDiffIsReset(diff: TimelineDiff): boolean {
  return diff === "Clear" || (typeof diff !== "string" && "Reset" in diff);
}

export function timelineDiffsContainReset(diffs: readonly TimelineDiff[]): boolean {
  return diffs.some(timelineDiffIsReset);
}

function timelineDiffItems(diff: TimelineDiff): TimelineItem[] {
  if (typeof diff === "string") {
    return [];
  }
  if ("PushFront" in diff) {
    return [diff.PushFront.item];
  }
  if ("PushBack" in diff) {
    return [diff.PushBack.item];
  }
  if ("Insert" in diff) {
    return [diff.Insert.item];
  }
  if ("Set" in diff) {
    return [diff.Set.item];
  }
  if ("Reset" in diff) {
    return diff.Reset.items;
  }
  return [];
}

function timelineItemIsOwnOutgoing(item: TimelineItem, currentUserId: string): boolean {
  return item.sender === currentUserId && item.send_state != null;
}

export function timelineRowsArePurePrepend(
  previousIds: readonly string[],
  nextIds: readonly string[]
): boolean {
  const added = nextIds.length - previousIds.length;
  return (
    added > 0 &&
    previousIds.length > 0 &&
    previousIds.every((id, index) => nextIds[added + index] === id)
  );
}

export const timelineRowsArePurePrependForTests = timelineRowsArePurePrepend;

export function latestEventBackedItemId(items: TimelineItem[]): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if ("Event" in item.id) {
      return item.id.Event.event_id;
    }
  }
  return null;
}

export function emitTimelineEventDiagnosticLog(
  event: TimelineEvent,
  key: TimelineKey,
  emit: (source: string, message: string) => void
): void {
  const kind = timelineKindDiagnosticLabel(key);
  if ("InitialItems" in event) {
    emit(
      "timeline.event",
      `kind=${kind} initial items=${event.InitialItems.items.length} generation=${event.InitialItems.generation}`
    );
    return;
  }
  if ("ItemsUpdated" in event) {
    emit(
      "timeline.event",
      `kind=${kind} update diffs=${event.ItemsUpdated.diffs.length} generation=${event.ItemsUpdated.generation}`
    );
    const linkPreviewSummary = timelineDiffLinkPreviewSummary(event.ItemsUpdated.diffs);
    if (linkPreviewSummary.items > 0) {
      emit(
        "timeline.preview",
        `kind=${kind} stage=update items=${linkPreviewSummary.items} pending=${linkPreviewSummary.pending} loading=${linkPreviewSummary.loading} ready=${linkPreviewSummary.ready} failed=${linkPreviewSummary.failed}`
      );
    }
    return;
  }
  if ("PaginationStateChanged" in event) {
    emit(
      "timeline.event",
      `kind=${kind} pagination direction=${event.PaginationStateChanged.direction} state=${paginationStateLogLabel(event.PaginationStateChanged.state)}`
    );
    return;
  }
  if ("AnchorRestoreFinished" in event) {
    emit(
      "timeline.event",
      `kind=${kind} anchor restore status=${anchorRestoreStatusLogLabel(event.AnchorRestoreFinished.status)}`
    );
    return;
  }
  if ("NavigationUpdated" in event) {
    emit(
      "timeline.event",
      `kind=${kind} navigation unread=${event.NavigationUpdated.snapshot.unread_event_count} newer=${event.NavigationUpdated.snapshot.newer_event_count} bottom=${event.NavigationUpdated.snapshot.can_jump_to_bottom}`
    );
    return;
  }
  if ("ResyncRequired" in event) {
    emit("timeline.event", `kind=${kind} resync reason=${event.ResyncRequired.reason}`);
  }
}

function timelineDiffLinkPreviewSummary(diffs: readonly TimelineDiff[]): {
  items: number;
  pending: number;
  loading: number;
  ready: number;
  failed: number;
} {
  const summary = {
    items: 0,
    pending: 0,
    loading: 0,
    ready: 0,
    failed: 0
  };
  for (const diff of diffs) {
    for (const item of timelineDiffItems(diff)) {
      const previews = item.link_previews ?? [];
      if (previews.length === 0) {
        continue;
      }
      summary.items += 1;
      for (const preview of previews) {
        summary[preview.state] += 1;
      }
    }
  }
  return summary;
}

export function timelineBackfillCompletionReason(event: TimelineEvent): string | null {
  if ("ResyncRequired" in event) {
    return "reset";
  }
  if ("PaginationStateChanged" in event) {
    if (
      event.PaginationStateChanged.direction !== "Backward" ||
      event.PaginationStateChanged.state === "Paginating"
    ) {
      return null;
    }
    return paginationStateBackfillCompletionReason(event.PaginationStateChanged.state);
  }
  return null;
}

function paginationStateBackfillCompletionReason(state: PaginationState): string {
  if (state === "Idle") {
    return "pagination_idle";
  }
  if (state === "EndReached") {
    return "pagination_end_reached";
  }
  return "pagination_failed";
}

export function timelineKindDiagnosticLabel(key: TimelineKey): "room" | "thread" | "focused" {
  if ("Room" in key.kind) {
    return "room";
  }
  if ("Thread" in key.kind) {
    return "thread";
  }
  return "focused";
}

function paginationStateLogLabel(state: PaginationState): string {
  if (typeof state === "string") {
    return state;
  }
  return `Failed(${state.Failed.kind})`;
}

function anchorRestoreStatusLogLabel(status: TimelineAnchorRestoreStatus): string {
  if (typeof status === "string") {
    return status;
  }
  return `Failed(${status.Failed.kind})`;
}

export function paginationStateDiagnosticLabel(
  state: ReturnType<typeof getPaginationState>
): string {
  if (typeof state === "string") {
    return state;
  }
  if ("Failed" in state) {
    return "Failed";
  }
  return "Unknown";
}
