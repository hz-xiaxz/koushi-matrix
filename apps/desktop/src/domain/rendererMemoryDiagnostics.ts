import type { TimelineStoreState } from "./timelineStore";
import { objectUrlDiagnostics } from "./objectUrlDiagnostics";

export interface RendererMemoryDiagnostics {
  domNodes: number;
  domElements: number;
  domTextUtf16BytesEstimate: number;
  imageNaturalRgbaBytesEstimate: number;
  uniqueImageNaturalRgbaBytesEstimate: number;
  uniqueImageSources: number;
  imagesWithNaturalSize: number;
  pendingImages: number;
  brokenImages: number;
  canvases: number;
  canvasRgbaBytesEstimate: number;
  objectUrls: number;
  objectUrlBlobBytes: number;
  timelineStoreKeys: number | null;
  timelineStoreItems: number | null;
  timelineStoreTextUtf16BytesEstimate: number | null;
}

const add = (a: number, b: number) => Math.min(Number.MAX_SAFE_INTEGER, a + b);
const dimension = (n: number) => Number.isFinite(n) ? Math.max(0, Math.trunc(n)) : 0;
const rgba = (width: number, height: number) =>
  Math.min(Number.MAX_SAFE_INTEGER, dimension(width) * dimension(height) * 4);

/** One export-time observation; no references to nodes, URLs or text escape. */
export function collectRendererMemoryDiagnostics(store?: TimelineStoreState): RendererMemoryDiagnostics {
  let domNodes = 0;
  let domElements = 0;
  let domTextUtf16BytesEstimate = 0;
  const walker = document.createTreeWalker(document, NodeFilter.SHOW_ALL);
  for (let node: Node | null = walker.currentNode; node; node = walker.nextNode()) {
    domNodes += 1;
    if (node.nodeType === Node.ELEMENT_NODE) domElements += 1;
    if (node.nodeType === Node.TEXT_NODE) {
      domTextUtf16BytesEstimate = add(domTextUtf16BytesEstimate, (node.nodeValue?.length ?? 0) * 2);
    }
  }
  let imageNaturalRgbaBytesEstimate = 0;
  let uniqueImageNaturalRgbaBytesEstimate = 0;
  let imagesWithNaturalSize = 0;
  let pendingImages = 0;
  let brokenImages = 0;
  const sources = new Map<string, number>();
  for (const image of Array.from(document.images)) {
    if (!image.complete) pendingImages += 1;
    const bytes = rgba(image.naturalWidth, image.naturalHeight);
    if (bytes === 0) {
      if (image.complete && (image.currentSrc || image.getAttribute("src"))) brokenImages += 1;
      continue;
    }
    imagesWithNaturalSize += 1;
    imageNaturalRgbaBytesEstimate = add(imageNaturalRgbaBytesEstimate, bytes);
    const source = image.currentSrc || image.src;
    if (source && bytes > (sources.get(source) ?? 0)) {
      sources.set(source, bytes);
    }
  }
  for (const bytes of sources.values()) uniqueImageNaturalRgbaBytesEstimate = add(uniqueImageNaturalRgbaBytesEstimate, bytes);
  const canvases = document.querySelectorAll("canvas");
  let canvasRgbaBytesEstimate = 0;
  for (const canvas of canvases) canvasRgbaBytesEstimate = add(canvasRgbaBytesEstimate, rgba(canvas.width, canvas.height));
  let timelineStoreItems: number | null = null;
  let timelineStoreTextUtf16BytesEstimate: number | null = null;
  if (store) {
    timelineStoreItems = 0;
    timelineStoreTextUtf16BytesEstimate = 0;
    for (const state of store.keys.values()) {
      timelineStoreItems += state.items.length;
      for (const item of state.items) {
        const chars = (item.body?.length ?? 0) + (item.formatted?.html.length ?? 0) +
          (item.formatted?.plain_text.length ?? 0);
        timelineStoreTextUtf16BytesEstimate = add(timelineStoreTextUtf16BytesEstimate, chars * 2);
      }
    }
  }
  return {
    domNodes, domElements, domTextUtf16BytesEstimate,
    imageNaturalRgbaBytesEstimate, uniqueImageNaturalRgbaBytesEstimate,
    uniqueImageSources: sources.size, imagesWithNaturalSize, pendingImages, brokenImages,
    canvases: canvases.length, canvasRgbaBytesEstimate,
    ...objectUrlDiagnostics(),
    timelineStoreKeys: store?.keys.size ?? null,
    timelineStoreItems, timelineStoreTextUtf16BytesEstimate
  };
}
