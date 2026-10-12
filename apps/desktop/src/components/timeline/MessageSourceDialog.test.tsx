// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MessageSourceDialog } from "./MessageSourceDialog";
import type { TimelineMessageSource } from "../../domain/coreEvents";
import { t } from "../../i18n/messages";

afterEach(cleanup);

const source: TimelineMessageSource = {
  event_id: "$source:example.invalid",
  sender: "@alice:example.invalid",
  timestamp_ms: 1_800_000_000_000,
  body: "synthetic body",
  in_reply_to_event_id: null,
  thread_root: null,
  is_redacted: false,
  is_edited: false,
  has_media: false,
  megolm_session_fingerprint: "AbCdEfGhIjKl"
};

function dialog(): HTMLElement | null {
  return screen.queryByRole("dialog", { name: t("timeline.messageSource") });
}

// #1368: the source panel renders as a non-modal floating surface without a
// backdrop, so a press outside it must close it instead of leaving it over the
// timeline.
describe("message source dialog dismissal", () => {
  it("closes on a press outside the panel", () => {
    const onClose = vi.fn();
    render(<MessageSourceDialog source={source} onClose={onClose} />);
    expect(dialog()).not.toBeNull();

    fireEvent.pointerDown(document.body);

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("stays open while the press lands inside the panel", () => {
    const onClose = vi.fn();
    render(<MessageSourceDialog source={source} onClose={onClose} />);

    fireEvent.pointerDown(dialog()!);

    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<MessageSourceDialog source={source} onClose={onClose} />);

    fireEvent.keyDown(dialog()!, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
