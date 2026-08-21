import type { SecurityDiagnostics } from "../domain/diagnostics";
import type { QaDomDiagnostics, QaTimelineDiagnostics } from "../domain/qaTitle";

export const INITIAL_TIMELINE_DIAGNOSTICS: QaTimelineDiagnostics = {
  visibleItems: 0,
  downloadedItems: 0,
  backfill: "unknown",
  avatarMxcItems: 0,
  avatarReadyItems: 0,
  avatarPendingItems: 0,
  avatarFailedItems: 0,
  avatarMissingItems: 0,
  avatarRenderedImages: 0,
  avatarBrokenImages: 0
};

export function qaRenderedDomDiagnostics(): QaDomDiagnostics {
  const root = document.getElementById("root");
  const screen = document.querySelector('[data-testid="boot-error"]')
    ? "boot_error"
    : document.querySelector('[data-testid="auth-screen"]')
      ? "auth"
      : document.querySelector('[data-testid="recovery-panel"]')
        ? "recovery"
        : document.querySelector('[data-testid="timeline-view"]')
          ? "timeline"
          : root?.childElementCount
            ? "unknown"
            : "empty";

  return {
    screen,
    rootChildren: root?.childElementCount ?? 0,
    bodyTextLength: (document.body.innerText ?? document.body.textContent ?? "").length
  };
}

export function qaSecurityDiagnostics(): SecurityDiagnostics {
  const avatarImages = Array.from(
    document.querySelectorAll<HTMLImageElement>(
      ".avatar img, .room-avatar img, .space-avatar img, .receipt-reader-avatar img"
    )
  );
  return {
    secureContext: window.isSecureContext,
    locationProtocol: window.location.protocol,
    locationOrigin: window.location.origin,
    avatarImageSchemes: avatarImages.reduce<Record<string, number>>((counts, image) => {
      const scheme = imageSrcScheme(image.currentSrc || image.src);
      counts[scheme] = (counts[scheme] ?? 0) + 1;
      return counts;
    }, {}),
    avatarBrokenImages: avatarImages.filter((image) => !image.complete || image.naturalWidth === 0)
      .length
  };
}

function imageSrcScheme(src: string): string {
  try {
    const protocol = new URL(src, window.location.href).protocol;
    return protocol.endsWith(":") ? protocol.slice(0, -1) : protocol;
  } catch {
    return "invalid";
  }
}

export function timelineDiagnosticsEqual(
  left: QaTimelineDiagnostics,
  right: QaTimelineDiagnostics
): boolean {
  return (
    left.visibleItems === right.visibleItems &&
    left.downloadedItems === right.downloadedItems &&
    left.backfill === right.backfill &&
    left.avatarMxcItems === right.avatarMxcItems &&
    left.avatarReadyItems === right.avatarReadyItems &&
    left.avatarPendingItems === right.avatarPendingItems &&
    left.avatarFailedItems === right.avatarFailedItems &&
    left.avatarMissingItems === right.avatarMissingItems &&
    left.avatarRenderedImages === right.avatarRenderedImages &&
    left.avatarBrokenImages === right.avatarBrokenImages
  );
}

export function timelineDiagnosticsLogMessage(diagnostics: QaTimelineDiagnostics): string {
  return [
    `items visible=${diagnostics.visibleItems}`,
    `downloaded=${diagnostics.downloadedItems}`,
    `backfill=${diagnostics.backfill}`,
    `avatars mxc=${diagnostics.avatarMxcItems}`,
    `ready=${diagnostics.avatarReadyItems}`,
    `pending=${diagnostics.avatarPendingItems}`,
    `failed=${diagnostics.avatarFailedItems}`,
    `missing=${diagnostics.avatarMissingItems}`,
    `rendered=${diagnostics.avatarRenderedImages}`,
    `broken=${diagnostics.avatarBrokenImages}`
  ].join(" ");
}
