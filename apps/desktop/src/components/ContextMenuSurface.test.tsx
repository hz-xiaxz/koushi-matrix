// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ContextMenuItem } from "../domain/contextMenus";
import { ContextMenuSurface, contextMenuPosition } from "./ContextMenuSurface";

afterEach(cleanup);

describe("ContextMenuSurface", () => {
  it("renders context menu items with destructive affordance and fixed position", () => {
    const markup = renderToStaticMarkup(
      <ContextMenuSurface
        items={[
          { id: "openThread", labelMessageId: "context.openThread" },
          { id: "redactMessage", labelMessageId: "context.redactMessage", destructive: true }
        ]}
        x={120}
        y={80}
        onAction={() => undefined}
        onClose={() => undefined}
      />
    );

    expect(markup).toContain('role="menu"');
    expect(markup).toContain("Reply in thread");
    expect(markup).toContain("Redact");
    expect(markup).toContain("context-menu-item destructive");
    expect(markup).toContain("left:120px");
    expect(markup).toContain("top:80px");
  });

  it("clamps context menus inside the viewport", () => {
    expect(
      contextMenuPosition({
        itemCount: 3,
        viewport: { height: 768, width: 1024 },
        x: 980,
        y: 740
      })
    ).toEqual({ left: 832, top: 656 });
  });

  describe("keyboard navigation (#480 audit)", () => {
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
});
