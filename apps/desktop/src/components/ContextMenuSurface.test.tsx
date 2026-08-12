// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ContextMenuItem } from "../domain/contextMenus";
import { ContextMenuSurface } from "./ContextMenuSurface";

afterEach(cleanup);

const items: ContextMenuItem[] = [
  { id: "editMessage", labelMessageId: "timeline.editMessage" },
  { id: "openThread", labelMessageId: "timeline.replyInThread" },
  { id: "redactMessage", labelMessageId: "timeline.redactMessage" }
];

function renderMenu(onAction = vi.fn(), onClose = vi.fn()) {
  const rendered = render(
    <ContextMenuSurface items={items} x={100} y={100} onAction={onAction} onClose={onClose} />
  );
  return { ...rendered, onAction, onClose };
}

function menuitems() {
  return screen.getAllByRole("menuitem") as HTMLButtonElement[];
}

describe("ContextMenuSurface keyboard navigation (#480 audit)", () => {
  it("focuses the first item when the menu opens", () => {
    renderMenu();
    expect(document.activeElement).toBe(menuitems()[0]);
  });

  it("moves focus with Arrow Down and Arrow Up, wrapping at both edges", () => {
    const { container } = renderMenu();
    const menu = container.querySelector(".context-menu") as HTMLElement;
    const [first, second, third] = menuitems();

    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(second);
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(third);
    // Wrap from the last item to the first.
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(first);
    // Wrap from the first item to the last.
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(document.activeElement).toBe(third);
  });

  it("jumps to the ends with Home and End", () => {
    const { container } = renderMenu();
    const menu = container.querySelector(".context-menu") as HTMLElement;
    const [first, , third] = menuitems();

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "End" });
    expect(document.activeElement).toBe(third);
    fireEvent.keyDown(menu, { key: "Home" });
    expect(document.activeElement).toBe(first);
  });

  it("activates the focused item and closes with Escape", () => {
    const { container, onAction, onClose } = renderMenu();
    const menu = container.querySelector(".context-menu") as HTMLElement;
    const [, second] = menuitems();

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(second);
    fireEvent.click(second);
    expect(onAction).toHaveBeenCalledWith("openThread");

    fireEvent.keyDown(menu, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
