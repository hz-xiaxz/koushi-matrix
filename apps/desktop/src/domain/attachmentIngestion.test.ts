import { readFileSync } from "node:fs";

import { describe, expect, it, vi } from "vitest";

import {
  attachmentPasteNeedsNativeImageRead,
  filesFromAttachmentTransfer,
  ingestAttachmentFiles,
  stageAttachmentFiles
} from "./attachmentIngestion";

describe("attachment ingestion", () => {
  it("preserves the incoming file order at the single staging boundary", async () => {
    const first = new File(["first"], "first.pdf", { type: "application/pdf" });
    const second = new File(["second"], "second.zip", { type: "application/zip" });
    const ingest = vi.fn(async (_files: File[]) => undefined);

    await ingestAttachmentFiles([first, second], ingest);

    expect(ingest).toHaveBeenCalledOnce();
    expect(ingest.mock.calls[0]?.[0]).toEqual([first, second]);
  });

  it("accepts only file-bearing browser transfers", () => {
    const file = new File(["data"], "document.pdf", { type: "application/pdf" });

    expect(
      filesFromAttachmentTransfer({
        files: [file],
        items: [{ kind: "file" }],
        types: ["Files"]
      })
    ).toEqual([file]);
    expect(
      filesFromAttachmentTransfer({
        files: [],
        items: [{ kind: "string" }],
        types: ["text/plain"]
      })
    ).toEqual([]);
  });

  it("asks the native clipboard only when the paste exposes neither files nor text", () => {
    const file = new File(["data"], "image.png", { type: "image/png" });
    // WebKitGTK reports an image-only clipboard as a completely empty transfer.
    expect(
      attachmentPasteNeedsNativeImageRead({ files: [], items: [], types: [], getData: () => "" })
    ).toBe(true);
    // "Copy image" from a browser adds markup but still no plain text to paste.
    expect(
      attachmentPasteNeedsNativeImageRead({
        files: [],
        items: [{ kind: "string" }],
        types: ["text/html"],
        getData: () => ""
      })
    ).toBe(true);
    expect(
      attachmentPasteNeedsNativeImageRead({
        files: [],
        items: [{ kind: "string" }],
        types: ["text/plain"],
        getData: () => "hello"
      })
    ).toBe(false);
    expect(
      attachmentPasteNeedsNativeImageRead({
        files: [file],
        items: [{ kind: "file" }],
        types: ["Files"],
        getData: () => ""
      })
    ).toBe(false);
  });

  it("keeps packaged desktop file drops on the browser File ingestion path", () => {
    const config = JSON.parse(
      readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8")
    ) as { app: { windows: Array<{ dragDropEnabled?: boolean }> } };

    expect(config.app.windows[0]?.dragDropEnabled).toBe(false);
  });

  it("enables native drag/drop only on Linux, where WebKitGTK hides dragged files", () => {
    const read = (name: string) =>
      JSON.parse(readFileSync(new URL(`../../src-tauri/${name}`, import.meta.url), "utf8")) as {
        app: { windows: Array<Record<string, unknown>> };
      };
    const base = read("tauri.conf.json").app.windows;
    const linux = read("tauri.linux.conf.json").app.windows;

    // The platform file replaces the whole windows array, so it must stay an
    // exact copy of the base window apart from the one Linux difference.
    expect(linux).toEqual([{ ...base[0], dragDropEnabled: true }]);
    expect(base).toHaveLength(1);
  });

  it("captures target and bytes immediately before staging", async () => {
    const first = new File([new Uint8Array([1, 2])], "first.pdf", {
      type: "application/pdf"
    });
    const second = new File([new Uint8Array([3])], "second.zip", {
      type: "application/zip"
    });
    const target = { kind: "thread", room_id: "room-a", root_event_id: "$root" } as const;
    const stage = vi.fn(async () => undefined);

    await stageAttachmentFiles(target, [first, second], 4, (position) => `stage-${position}`, stage);

    expect(stage).toHaveBeenCalledWith(target, [
      expect.objectContaining({ stagedId: "stage-5", position: 5, bytes: [1, 2] }),
      expect.objectContaining({ stagedId: "stage-6", position: 6, bytes: [3] })
    ]);
  });
});
