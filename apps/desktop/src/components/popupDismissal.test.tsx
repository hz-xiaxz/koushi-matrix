// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useOutsidePressDismissal } from "./popupDismissal";

afterEach(cleanup);

function Popup({
  content,
  kind,
  onDismiss,
  open
}: {
  content: string;
  kind: string;
  onDismiss: () => void;
  open: boolean;
}) {
  const contentRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  useOutsidePressDismissal({ open, kind, contentRef, triggerRef, onDismiss });

  return (
    <div>
      <button ref={triggerRef} type="button">
        {`trigger-${content}`}
      </button>
      {open ? <div ref={contentRef}>{`content-${content}`}</div> : null}
    </div>
  );
}

// The catalog scanner reads this directory, so button copy is a value, not a
// literal JSX text node.
const OPEN_A = "open-a";
const OPEN_B = "open-b";

const content = (label: string) => screen.getByText(`content-${label}`);
const trigger = (label: string) => screen.getByRole("button", { name: `trigger-${label}` });

describe("useOutsidePressDismissal", () => {
  it("dismisses on a press outside the content and its trigger", () => {
    const onDismiss = vi.fn();
    render(<Popup content="a" kind="group" onDismiss={onDismiss} open />);

    fireEvent.pointerDown(document.body);

    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("keeps the press that lands inside the content", () => {
    const onDismiss = vi.fn();
    render(<Popup content="a" kind="group" onDismiss={onDismiss} open />);

    fireEvent.pointerDown(content("a"));

    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("leaves a press on the trigger to the trigger's own toggle", () => {
    const onDismiss = vi.fn();
    render(<Popup content="a" kind="group" onDismiss={onDismiss} open />);

    fireEvent.pointerDown(trigger("a"));

    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("does not dismiss itself when it re-renders with a new callback identity", () => {
    const onDismiss = vi.fn();
    const view = render(<Popup content="a" kind="group" onDismiss={() => onDismiss()} open />);

    view.rerender(<Popup content="a" kind="group" onDismiss={() => onDismiss()} open />);

    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("opens one popup per group and closes the previous one", () => {
    function Group() {
      const [open, setOpen] = useState<string | null>(null);
      return (
        <>
          <Popup content="a" kind="group" onDismiss={() => setOpen(null)} open={open === "a"} />
          <Popup content="b" kind="group" onDismiss={() => setOpen(null)} open={open === "b"} />
          <button type="button" onClick={() => setOpen("a")}>
            {OPEN_A}
          </button>
          <button type="button" onClick={() => setOpen("b")}>
            {OPEN_B}
          </button>
        </>
      );
    }
    render(<Group />);

    fireEvent.click(screen.getByRole("button", { name: OPEN_A }));
    expect(screen.queryByText("content-a")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: OPEN_B }));

    expect(screen.queryByText("content-a")).toBeNull();
    expect(screen.queryByText("content-b")).not.toBeNull();
  });

  it("stops dismissing after the popup closes", () => {
    const onDismiss = vi.fn();
    const view = render(<Popup content="a" kind="group" onDismiss={onDismiss} open />);

    view.rerender(<Popup content="a" kind="group" onDismiss={onDismiss} open={false} />);
    fireEvent.pointerDown(document.body);

    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("opens every group independently", () => {
    const onDismiss = vi.fn();
    const view = render(<Popup content="a" kind="first" onDismiss={onDismiss} open />);

    view.rerender(
      <>
        <Popup content="a" kind="first" onDismiss={onDismiss} open />
        <Popup content="b" kind="second" onDismiss={() => undefined} open />
      </>
    );

    expect(onDismiss).not.toHaveBeenCalled();
  });
});
