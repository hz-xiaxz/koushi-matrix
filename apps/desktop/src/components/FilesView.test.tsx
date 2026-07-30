// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FilesView } from "./FilesView";

afterEach(cleanup);

describe("FilesView", () => {
  it("uses the projected sender label and never exposes a raw Matrix id as metadata", () => {
    render(
      <FilesView
        filesView={{
          kind: "open",
          request_id: 1,
          scope: { kind: "account" },
          filter: {
            kinds: ["file"],
            filename_query: null
          },
          sort: "newestFirst",
          items: [
            {
              room_id: "!room:example.invalid",
              event_id: "$file:example.invalid",
              sender: "@private:example.invalid",
              sender_label: null,
              timestamp_ms: 1_800_000_000_000,
              kind: "file",
              filename: "notes.txt",
              mimetype: "text/plain",
              size: 12,
              source_mxc: "mxc://example.invalid/file",
              thumbnail_mxc: null,
              thread_root: null,
              encrypted: false,
              encryption_version: null,
              width: null,
              height: null,
              is_edited: false
            }
          ],
          selected_event_id: null
        }}
        onChangeFilterSort={() => undefined}
      />
    );

    expect(screen.getByText(/Unknown user/)).toBeTruthy();
    expect(screen.queryByText(/@private:example\.invalid/)).toBeNull();
  });

  it("does not apply a filename search for IME candidate-confirmation Enter", () => {
    const onChangeFilterSort = vi.fn();
    render(
      <FilesView
        filesView={{
          kind: "open",
          request_id: 1,
          scope: { kind: "account" },
          filter: {
            kinds: ["image", "video", "audio", "file", "sticker"],
            filename_query: null
          },
          sort: "newestFirst",
          items: [],
          selected_event_id: null
        }}
        onChangeFilterSort={onChangeFilterSort}
      />
    );
    const search = screen.getByRole("searchbox") as HTMLInputElement;

    fireEvent.compositionStart(search);
    fireEvent.change(search, { target: { value: "日本語" } });
    fireEvent.keyDown(search, {
      key: "Enter",
      code: "Enter",
      keyCode: 229,
      isComposing: true
    });

    expect(onChangeFilterSort).not.toHaveBeenCalled();
  });
});
