import { describe, expect, test } from "vitest";

import type { TimelineItem } from "./coreEvents";
import { projectTimelineDisplayRows } from "./timelineDisplayProjection";

function message(id: string, timestamp = 1): TimelineItem {
  return {
    id: { Event: { event_id: id } },
    sender: "@sender:example.invalid",
    body: id,
    timestamp_ms: timestamp,
    in_reply_to_event_id: null,
    thread_root: null,
    thread_summary: null,
    reactions: [],
    can_react: false,
    is_redacted: false,
    is_hidden: false,
    can_redact: false,
    is_edited: false,
    can_edit: false,
    display_metadata: null
  };
}

describe("Rust timeline display adapter", () => {
  test("preserves Rust-provided order and stable metadata", () => {
    const first = message("$first");
    const root = {
      ...message("$root", 2),
      display_metadata: {
        row_id: "thread-root:$root",
        kind: { kind: "threadRoot" as const },
        content_event_id: "$root",
        activity_event_id: "$reply",
        display_timestamp_ms: 9
      }
    };
    const second = message("$second", 3);

    const eventRows = projectTimelineDisplayRows([second, root, first]).filter(
      (row) => row.kind !== "dateDivider"
    );
    expect(eventRows.map((row) => row.row_id)).toEqual([
      "$second",
      "thread-root:$root",
      "$first"
    ]);
    expect(eventRows[1]).toMatchObject({
      content_event_id: "$root",
      activity_event_id: "$reply",
      display_timestamp_ms: 9,
      kind: "threadRoot"
    });
  });

  test("maps pending and failed Rust rows without synthesizing lifecycle state", () => {
    const pending = {
      ...message("$pending"),
      id: { Synthetic: { synthetic_id: "thread-root-slot:$root" } },
      display_metadata: {
        row_id: "thread-root:$root",
        kind: { kind: "threadRootPending" as const },
        content_event_id: "$root",
        activity_event_id: "$reply",
        display_timestamp_ms: 4
      }
    };
    const failed = {
      ...message("$failed"),
      display_metadata: {
        row_id: "thread-root:$failed",
        kind: { kind: "threadRootFailed" as const, failure_kind: "sdk" as const },
        content_event_id: "$failed",
        activity_event_id: "$reply-failed",
        display_timestamp_ms: 5
      }
    };

    const projectionRows = projectTimelineDisplayRows([pending, failed]).filter(
      (row) => row.kind !== "dateDivider"
    );
    expect(projectionRows.map((row) => row.kind)).toEqual([
      "threadRootPending",
      "threadRootFailed"
    ]);
    expect(projectionRows[0]?.item.id).toEqual({
      Synthetic: { synthetic_id: "thread-root-slot:$root" }
    });
  });

  test("preserves renderer date dividers without changing Rust event order", () => {
    const first = message("$a", Date.UTC(2027, 0, 1));
    const second = message("$b", Date.UTC(2027, 0, 2));
    const canonicalDivider = {
      ...message("$divider", Date.UTC(2027, 0, 2)),
      id: { Synthetic: { synthetic_id: "date-divider-input" } }
    };
    const rows = projectTimelineDisplayRows([second, canonicalDivider, first]);
    expect(rows.map((row) => row.kind)).toEqual(["event", "dateDivider", "event"]);
    expect(rows.filter((row) => row.kind === "event").map((row) => row.row_id)).toEqual([
      "$b",
      "$a"
    ]);
  });
});
