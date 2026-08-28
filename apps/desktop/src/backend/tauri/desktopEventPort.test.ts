/* @vitest-environment jsdom */

import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, test, vi } from "vitest";

import type { CoreEventPayload } from "../../domain/coreEvents";
import { createTauriDesktopEventPort } from "./desktopEventPort";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const unlisten = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listen).mockResolvedValue(unlisten);
});

describe("Tauri desktop event port", () => {
  test("constructs without subscribing", () => {
    createTauriDesktopEventPort();
    expect(listen).not.toHaveBeenCalled();
  });

  test("unwraps Core and menu payloads on their exact channels", async () => {
    const coreListener = vi.fn();
    const menuListener = vi.fn();
    const port = createTauriDesktopEventPort();

    await expect(port.listenCoreEvents(coreListener)).resolves.toBe(unlisten);
    await expect(port.listenMenuActions(menuListener)).resolves.toBe(unlisten);

    expect(listen).toHaveBeenNthCalledWith(
      1,
      "koushi-desktop://event",
      expect.any(Function)
    );
    expect(listen).toHaveBeenNthCalledWith(
      2,
      "koushi-desktop://menu",
      expect.any(Function)
    );
    const corePayload = { kind: "ResyncMarker", generation: 7 } as CoreEventPayload;
    const coreEnvelopeListener = vi.mocked(listen).mock.calls[0]?.[1] as (
      event: { payload: CoreEventPayload }
    ) => void;
    const menuEnvelopeListener = vi.mocked(listen).mock.calls[1]?.[1] as (
      event: { payload: string }
    ) => void;
    coreEnvelopeListener({ payload: corePayload });
    menuEnvelopeListener({ payload: "toggleFullscreen" });

    expect(coreListener).toHaveBeenCalledWith(corePayload);
    expect(menuListener).toHaveBeenCalledWith("toggleFullscreen");
  });

  test("ignores the state payload and returns the Tauri disposer", async () => {
    const stateListener = vi.fn();
    const port = createTauriDesktopEventPort();

    await expect(port.listenStateChanges(stateListener)).resolves.toBe(unlisten);
    expect(listen).toHaveBeenCalledWith("koushi-desktop://state", expect.any(Function));
    const envelopeListener = vi.mocked(listen).mock.calls[0]?.[1] as (
      event: { payload: string }
    ) => void;
    envelopeListener({ payload: "private ignored payload" });

    expect(stateListener).toHaveBeenCalledOnce();
    expect(stateListener).toHaveBeenCalledWith();
  });
});
