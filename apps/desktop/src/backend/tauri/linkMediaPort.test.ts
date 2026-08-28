/* @vitest-environment jsdom */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, describe, expect, it, vi } from "vitest";

import { tauriLinkMediaPort } from "./linkMediaPort";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(async (command: string) =>
    command === "default_media_save_path" ? "/downloads/default.png" : undefined
  )
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(async () => "/downloads/chosen.png")
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => undefined)
}));

afterEach(() => {
  vi.clearAllMocks();
});

describe("Tauri link/media port", () => {
  it("opens through Tauri and falls back to an isolated browser window", async () => {
    await tauriLinkMediaPort.openHttpUrl("https://example.com/");
    expect(openUrl).toHaveBeenCalledWith("https://example.com/");

    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);
    vi.mocked(openUrl).mockRejectedValueOnce(new Error("unavailable"));
    await tauriLinkMediaPort.openHttpUrl("https://fallback.example/");
    expect(windowOpen).toHaveBeenCalledWith(
      "https://fallback.example/",
      "_blank",
      "noopener,noreferrer"
    );
  });

  it("passes web URLs through and converts local media paths", () => {
    expect(tauriLinkMediaPort.mediaSourceUrl("https://example.invalid/image.png")).toBe(
      "https://example.invalid/image.png"
    );
    expect(tauriLinkMediaPort.mediaSourceUrl("asset://localhost/avatar.png")).toBe(
      "asset://localhost/avatar.png"
    );
    expect(tauriLinkMediaPort.mediaSourceUrl("koushi-thumbnail://localhost/avatar/abc123")).toBe(
      "koushi-thumbnail://localhost/avatar/abc123"
    );
    expect(tauriLinkMediaPort.mediaSourceUrl("file:///tmp/avatar%20image.png")).toBe(
      "asset:///tmp/avatar image.png"
    );
    expect(convertFileSrc).toHaveBeenCalledWith("/tmp/avatar image.png");
    expect(tauriLinkMediaPort.mediaSourceUrl("/tmp/media-downloads/report.pdf")).toBe(
      "asset:///tmp/media-downloads/report.pdf"
    );
  });

  it("preserves the default-path, dialog and save command contract", async () => {
    await tauriLinkMediaPort.saveMediaFile("asset://media", ' report:*?.png ');

    expect(invoke).toHaveBeenCalledWith("default_media_save_path", {
      filename: "report_.png"
    });
    expect(saveDialog).toHaveBeenCalledWith({
      title: "Download report_.png",
      defaultPath: "/downloads/default.png"
    });
    expect(invoke).toHaveBeenCalledWith("save_downloaded_media", {
      sourceUrl: "asset://media",
      destinationPath: "/downloads/chosen.png"
    });
  });

  it("does not save when the dialog is cancelled", async () => {
    vi.mocked(saveDialog).mockResolvedValueOnce(null);
    await tauriLinkMediaPort.saveMediaFile("asset://media", "media.png");

    expect(invoke).not.toHaveBeenCalledWith("save_downloaded_media", expect.anything());
  });
});
