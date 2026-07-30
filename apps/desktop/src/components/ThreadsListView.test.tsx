// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ThreadsListView } from "./ThreadsListView";

afterEach(cleanup);

describe("ThreadsListView", () => {
  it("uses friendly fallbacks instead of raw Matrix ids for sender metadata", () => {
    render(
      <ThreadsListView
        roomId="!room:example.invalid"
        threadsList={{
          kind: "open",
          room_id: "!room:example.invalid",
          request_id: 1,
          items: [
            {
              root_event_id: "$root:example.invalid",
              root_sender: "@private-root:example.invalid",
              root_sender_label: null,
              root_body_preview: "Root",
              root_timestamp_ms: 1_800_000_000_000,
              latest_event_id: "$reply:example.invalid",
              latest_sender: "@private-reply:example.invalid",
              latest_sender_label: null,
              latest_body_preview: "Reply",
              latest_timestamp_ms: 1_800_000_000_100,
              reply_count: 1
            }
          ],
          is_paginating: false,
          end_reached: true
        }}
        onClose={() => undefined}
        onOpenThread={() => undefined}
        onPaginate={() => undefined}
      />
    );

    expect(screen.getAllByText(/Unknown user/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/@private-(root|reply):example\.invalid/)).toBeNull();
  });
});
