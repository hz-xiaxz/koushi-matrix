/* @vitest-environment jsdom */

import { afterEach, expect, test, vi } from "vitest";
import { listenForAppShortcuts } from "./keyboardShortcuts";

let dispose = () => {};
afterEach(() => {
  dispose();
  document.body.replaceChildren();
});

function press(target: EventTarget, key: string, ctrlKey = true) {
  const event = new KeyboardEvent("keydown", {
    key, metaKey: true, ctrlKey, bubbles: true, cancelable: true
  });
  target.dispatchEvent(event);
  return event;
}

test("fullscreen works while a modal dialog is open", () => {
  const handle = vi.fn(() => true);
  dispose = listenForAppShortcuts(handle);
  const dialog = document.createElement("dialog");
  dialog.open = true;
  document.body.append(dialog);

  expect(press(dialog, "f").defaultPrevented).toBe(true);
  expect(handle).toHaveBeenCalledExactlyOnceWith("toggleFullscreen");
});

test("fullscreen reaches the window before a focused surface stops key propagation", () => {
  const handle = vi.fn(() => true);
  dispose = listenForAppShortcuts(handle);
  const surface = document.createElement("div");
  surface.tabIndex = 0;
  surface.addEventListener("keydown", (event) => event.stopPropagation());
  document.body.append(surface);
  surface.focus();

  expect(press(surface, "f").defaultPrevented).toBe(true);
  expect(handle).toHaveBeenCalledExactlyOnceWith("toggleFullscreen");
});

test.each([["-", "zoomOut"], ["+", "zoomIn"], ["=", "zoomIn"], ["0", "resetZoom"]])(
  "%s zoom works inside a modal whose content stops key propagation",
  (key, action) => {
    const handle = vi.fn(() => true);
    dispose = listenForAppShortcuts(handle);
    const dialog = document.createElement("dialog");
    dialog.open = true;
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    document.body.append(dialog);

    expect(press(dialog, key, false).defaultPrevented).toBe(true);
    expect(handle).toHaveBeenCalledExactlyOnceWith(action);
  }
);

test("unhandled window shortcuts retain the host default and are not dispatched twice", () => {
  const handle = vi.fn(() => false);
  dispose = listenForAppShortcuts(handle);
  expect(press(document.body, "+", false).defaultPrevented).toBe(false);
  expect(handle).toHaveBeenCalledExactlyOnceWith("zoomIn");
});

test("one fullscreen keypress dispatches once and cleanup removes every listener", () => {
  const handle = vi.fn(() => true);
  dispose = listenForAppShortcuts(handle);
  press(document.body, "f");
  expect(handle).toHaveBeenCalledExactlyOnceWith("toggleFullscreen");
  dispose();
  press(document.body, "f");
  expect(handle).toHaveBeenCalledTimes(1);
});

test("Ctrl+wheel shares zoom handling, ordinary scrolling and cleanup retain browser behavior", () => {
  const handle = vi.fn(() => true);
  dispose = listenForAppShortcuts(handle);
  function wheel(ctrlKey: boolean, deltaY: number) {
    const event = new WheelEvent("wheel", { ctrlKey, deltaY, bubbles: true, cancelable: true });
    document.body.dispatchEvent(event);
    return event.defaultPrevented;
  }
  expect(wheel(false, 1)).toBe(false);
  expect(wheel(true, 0)).toBe(false);
  expect(handle).not.toHaveBeenCalled();
  expect(wheel(true, -1)).toBe(true);
  expect(wheel(true, 1)).toBe(true);
  expect(handle.mock.calls).toEqual([["zoomIn"], ["zoomOut"]]);
  dispose();
  expect(wheel(true, -1)).toBe(false);
  expect(handle).toHaveBeenCalledTimes(2);
});

test("room search still respects modal and previously consumed key events", () => {
  const handle = vi.fn(() => true);
  dispose = listenForAppShortcuts(handle);
  const dialog = document.createElement("dialog");
  dialog.open = true;
  document.body.append(dialog);
  press(dialog, "f", false);
  expect(handle).not.toHaveBeenCalled();
  dialog.remove();
  const consumed = new KeyboardEvent("keydown", {
    key: "f", metaKey: true, bubbles: true, cancelable: true
  });
  consumed.preventDefault();
  document.body.dispatchEvent(consumed);
  expect(handle).not.toHaveBeenCalled();
  press(document.body, "f", false);
  expect(handle).toHaveBeenCalledExactlyOnceWith("searchInRoom");
});
