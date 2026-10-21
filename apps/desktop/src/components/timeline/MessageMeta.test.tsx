// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { memo } from "react";
import { ActiveLocaleContext } from "../../i18n/activeLocaleContext";
import { setActiveLocaleProfile } from "../../i18n/messages";
import { MessageMeta } from "./MessageMeta";

afterEach(() => { cleanup(); vi.useRealTimers(); setActiveLocaleProfile("en"); });

describe("MessageMeta sender profile action", () => {
  const displayLabel = ["Duplicate", "Name"].join(" ");
  const baseProps = {
    senderDisplayLabel: displayLabel,
    timestampMs: 1_800_000_000_000,
    isEdited: false,
    isRedacted: false,
    sendStateKind: null
  };

  it("uses a native labelled button and contains activation", () => {
    const onOpenSenderProfile = vi.fn();
    const onParentClick = vi.fn();
    render(
      <div onClick={onParentClick}>
        <MessageMeta {...baseProps} onOpenSenderProfile={onOpenSenderProfile} />
      </div>
    );

    const sender = screen.getByRole("button", {
      name: `Open profile for ${displayLabel}`
    });
    expect(sender.textContent).toBe(displayLabel);
    expect(sender.getAttribute("type")).toBe("button");
    fireEvent.click(sender);
    expect(onOpenSenderProfile).toHaveBeenCalledTimes(1);
    expect(onParentClick).not.toHaveBeenCalled();
  });

  it("keeps the sender as plain text without a bound stable-id action", () => {
    const { container } = render(<MessageMeta {...baseProps} />);
    expect(screen.queryByRole("button", { name: /Open profile for/ })).toBeNull();
    expect(container.querySelector("span.sender")?.textContent).toBe(displayLabel);
  });
});


describe("MessageMeta timestamp context", () => {
  const props = { senderDisplayLabel: "Alice", isEdited: false, isRedacted: false, sendStateKind: null };

  it("shows the date with older messages and exposes the full date on keyboard focus", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 21, 12));
    const timestamp = new Date(2026, 8, 1, 9, 30);
    const { container } = render(<MessageMeta {...props} timestampMs={timestamp.getTime()} />);
    const time = container.querySelector("time")!;
    expect(time.textContent).toContain("Sep 1");
    expect(time.getAttribute("datetime")).toBe(timestamp.toISOString());
    expect(time.tabIndex).toBe(0);
    fireEvent.focus(time);
    expect(screen.getByRole("tooltip").textContent).toContain("Tuesday, September 1, 2026");
    fireEvent.blur(time);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("updates at local midnight without a parent render, and cancels its presentation timer", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 21, 23, 59, 59));
    const { container, unmount } = render(<MessageMeta {...props} timestampMs={new Date(2026, 9, 21, 9).getTime()} />);
    expect(container.querySelector("time")?.textContent).not.toContain("Yesterday");
    act(() => { vi.advanceTimersByTime(1000); });
    expect(container.querySelector("time")?.textContent).toContain("Yesterday");
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("refreshes stale calendar labels when the window regains focus", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 21, 12));
    const { container } = render(<MessageMeta {...props} timestampMs={new Date(2026, 9, 21, 9).getTime()} />);
    vi.setSystemTime(new Date(2026, 9, 22, 12));
    fireEvent.focus(window);
    expect(container.querySelector("time")?.textContent).toContain("Yesterday");
  });

  it("omits unavailable timestamps", () => {
    const { container } = render(<MessageMeta {...props} timestampMs={null} />);
    expect(container.querySelector("time")).toBeNull();
  });
});


it("updates an unchanged memoized timestamp when the active locale changes", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 21, 12));
  const MemoMeta = memo(MessageMeta);
  const props = { senderDisplayLabel: "Alice", timestampMs: new Date(2026, 9, 20, 9).getTime(), isEdited: false, isRedacted: false, sendStateKind: null };
  const { container, rerender } = render(<ActiveLocaleContext value="en:none"><MemoMeta {...props} /></ActiveLocaleContext>);
  expect(container.querySelector("time")?.textContent).toContain("Yesterday");
  setActiveLocaleProfile("ja");
  rerender(<ActiveLocaleContext value="ja:none"><MemoMeta {...props} /></ActiveLocaleContext>);
  expect(container.querySelector("time")?.textContent).toContain("昨日");
});

it("shares one presentation timer across rows and releases it after the final row unmounts", () => {
  vi.useFakeTimers();
  const props = { senderDisplayLabel: "Alice", timestampMs: 0, isEdited: false, isRedacted: false, sendStateKind: null };
  const first = render(<MessageMeta {...props} />);
  const second = render(<MessageMeta {...props} />);
  expect(vi.getTimerCount()).toBe(1);
  first.unmount();
  expect(vi.getTimerCount()).toBe(1);
  second.unmount();
  expect(vi.getTimerCount()).toBe(0);
});
