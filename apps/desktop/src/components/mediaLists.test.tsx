// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MessageArticle, PinnedEventsList } from "./mediaLists";

afterEach(cleanup);

describe("people-facing media list labels", () => {
  it("renders pinned-event sender_label and hides the raw sender id", () => {
    render(
      <PinnedEventsList
        roomId="!room:example.invalid"
        pinnedEvents={[
          {
            event_id: "$pinned:example.invalid",
            sender: "@private:example.invalid",
            sender_label: "Pinned Alias",
            body_preview: "Pinned body",
            redacted: false
          }
        ]}
        onUnpin={() => undefined}
      />
    );

    expect(screen.getByText("Pinned Alias")).toBeTruthy();
    expect(screen.queryByText("@private:example.invalid")).toBeNull();
  });

  it("renders a profile label in fixture message metadata and fails closed when absent", () => {
    const message = {
      room_id: "!room:example.invalid",
      event_id: "$message:example.invalid",
      sender: "@private:example.invalid",
      timestamp_ms: 1_800_000_000_000,
      body: "Body",
      attachment_filename: null,
      reply_count: 0
    };
    const props = {
      currentUserId: null,
      message,
      query: "",
      onEditMessage: vi.fn(),
      onOpenThread: vi.fn(),
      onRedactMessage: vi.fn(),
      isIgnored: false
    };
    const { rerender } = render(
      <MessageArticle
        {...props}
        profileUsers={{
          [message.sender]: {
            user_id: message.sender,
            display_name: "Profile Name",
            display_label: "Local Alias",
            original_display_label: "Profile Name",
            mention_search_terms: [],
            avatar: null
          }
        }}
      />
    );

    expect(screen.getByText("Local Alias")).toBeTruthy();
    expect(screen.queryByText(message.sender)).toBeNull();

    rerender(<MessageArticle {...props} profileUsers={{}} />);
    expect(screen.getByText("Unknown user")).toBeTruthy();
    expect(screen.queryByText(message.sender)).toBeNull();
  });
});
