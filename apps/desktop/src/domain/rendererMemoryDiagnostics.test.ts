// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import { collectRendererMemoryDiagnostics } from "./rendererMemoryDiagnostics";
import { createTrackedObjectURL, revokeTrackedObjectURL } from "./objectUrlDiagnostics";
import { createTimelineStore } from "./timelineStore";
import type { TimelineKeyState } from "./timelineStore";

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

test("samples image and canvas reference sizes without exporting content or double-counting sources", () => {
  const first = document.createElement("img");
  first.src = "https://example.invalid/private-image";
  const duplicate = first.cloneNode() as HTMLImageElement;
  for (const image of [first, duplicate]) {
    Object.defineProperties(image, {
      naturalWidth: { value: 100 }, naturalHeight: { value: 50 }, complete: { value: true }
    });
  }
  const canvas = document.createElement("canvas");
  canvas.width = 20;
  canvas.height = 10;
  document.body.append(first, duplicate, canvas, document.createTextNode("private body"));
  const sample = collectRendererMemoryDiagnostics();
  expect(sample.imageNaturalRgbaBytesEstimate).toBe(40_000);
  expect(sample.uniqueImageNaturalRgbaBytesEstimate).toBe(20_000);
  expect(sample.uniqueImageSources).toBe(1);
  expect(sample.canvasRgbaBytesEstimate).toBe(800);
  expect(sample.domTextUtf16BytesEstimate).toBeGreaterThanOrEqual(24);
  const encoded = JSON.stringify(sample);
  expect(encoded).not.toContain("private-image");
  expect(encoded).not.toContain("private body");
  document.body.innerHTML = "";
  expect(sample.uniqueImageNaturalRgbaBytesEstimate).toBe(20_000);
});

test("accounts live object URLs until their existing owner revokes them", () => {
  vi.spyOn(URL, "createObjectURL").mockReturnValueOnce("blob:fixture-one").mockReturnValueOnce("blob:fixture-two");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  const before = collectRendererMemoryDiagnostics();
  const first = createTrackedObjectURL(new Blob([new Uint8Array(100)]));
  const second = createTrackedObjectURL(new Blob([new Uint8Array(250)]));
  expect(collectRendererMemoryDiagnostics().objectUrlBlobBytes - before.objectUrlBlobBytes).toBe(350);
  revokeTrackedObjectURL(first);
  revokeTrackedObjectURL(first);
  expect(collectRendererMemoryDiagnostics().objectUrlBlobBytes - before.objectUrlBlobBytes).toBe(250);
  revokeTrackedObjectURL(second);
  expect(collectRendererMemoryDiagnostics().objectUrls).toBe(before.objectUrls);
});

test("separates missing store measurements from an empty store and counts text per reference", () => {
  expect(collectRendererMemoryDiagnostics().timelineStoreItems).toBeNull();
  const store = createTimelineStore();
  expect(collectRendererMemoryDiagnostics(store).timelineStoreItems).toBe(0);
  const sharedItem = { body: "AB", formatted: { html: "<b>AB</b>", plain_text: "AB" } };
  store.keys.set("synthetic", { items: [sharedItem, sharedItem] } as unknown as TimelineKeyState);
  const sample = collectRendererMemoryDiagnostics(store);
  expect(sample.timelineStoreKeys).toBe(1);
  expect(sample.timelineStoreItems).toBe(2);
  expect(sample.timelineStoreTextUtf16BytesEstimate).toBe((2 + 9 + 2) * 2 * 2);
});

test("rejects non-finite image dimensions without claiming measured image memory", () => {
  const image = document.createElement("img");
  Object.defineProperties(image, { naturalWidth: { value: Infinity }, naturalHeight: { value: 10 } });
  document.body.append(image);
  const sample = collectRendererMemoryDiagnostics();
  expect(sample.imageNaturalRgbaBytesEstimate).toBe(0);
  expect(sample.imagesWithNaturalSize).toBe(0);
});
