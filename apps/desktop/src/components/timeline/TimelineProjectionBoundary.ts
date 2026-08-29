import { Component, type ReactNode } from "react";
import type { TimelineDisplayRow } from "../../domain/timelineDisplayProjection";
import type { ScrollAnchor } from "./TimelineViewportAnchors";

export type TimelineProjectionSnapshot = {
  timelineKeyHash: string;
  generation: number;
  signature: string;
  rows: readonly TimelineDisplayRow[];
};

export type PendingProjectionLayoutTransaction = {
  timelineKeyHash: string;
  generation: number;
  signature: string;
  viewportEpoch: number;
  mode: "free-scroll" | "live-edge";
  anchor: ScrollAnchor | null;
};

type ProjectionSnapshotBoundaryProps = {
  snapshot: TimelineProjectionSnapshot;
  onBeforeProjectionChange: (
    previous: TimelineProjectionSnapshot,
    next: TimelineProjectionSnapshot
  ) => void;
  children: ReactNode;
};

/**
 * Function components do not expose `getSnapshotBeforeUpdate`. This boundary
 * provides the commit-safe pre-mutation point needed to capture an old DOM
 * anchor without allowing an abandoned render to touch scroll transaction
 * refs. It renders no DOM of its own.
 */
export class ProjectionSnapshotBoundary extends Component<ProjectionSnapshotBoundaryProps> {
  override getSnapshotBeforeUpdate(previousProps: ProjectionSnapshotBoundaryProps): null {
    this.props.onBeforeProjectionChange(previousProps.snapshot, this.props.snapshot);
    return null;
  }

  override componentDidUpdate(): void {}

  override render(): ReactNode {
    return this.props.children;
  }
}


export function timelineProjectionSignature(rows: readonly TimelineDisplayRow[]): string {
  return rows
    .map((row) =>
      [
        row.row_id,
        row.kind,
        row.content_event_id ?? "",
        row.activity_event_id ?? "",
        row.display_timestamp_ms ?? ""
      ].join("\u0000")
    )
    .join("\u0001");
}

export function projectionStructureChanged(
  previous: TimelineProjectionSnapshot,
  next: TimelineProjectionSnapshot
): boolean {
  return previous.signature !== next.signature;
}

/**
 * Pick only ordinary rows that survive a projection change with both
 * identities intact. Thread-root rows are intentionally excluded even when
 * their row id survives: their visual placement is the mutation in progress.
 */
export function stableProjectionAnchorRowIds(
  previousRows: readonly TimelineDisplayRow[],
  nextRows: readonly TimelineDisplayRow[]
): ReadonlySet<string> {
  const nextByRowId = new Map(nextRows.map((row) => [row.row_id, row]));
  const stable = new Set<string>();
  for (const previous of previousRows) {
    const next = nextByRowId.get(previous.row_id);
    if (
      next === undefined ||
      previous.kind === "threadRoot" ||
      next.kind === "threadRoot" ||
      previous.content_event_id !== next.content_event_id ||
      previous.activity_event_id !== next.activity_event_id
    ) {
      continue;
    }
    stable.add(previous.row_id);
  }
  return stable;
}

