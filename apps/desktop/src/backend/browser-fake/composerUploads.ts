import type {
  ComposerState,
  ComposerTarget,
  DesktopSnapshot,
  PreparedUploadVariant,
  StageUploadBytesRequestItem,
  StagedUploadItem
} from "../../domain/types";

export function browserComposerTargetIsActive(snapshot: DesktopSnapshot, target: ComposerTarget): boolean {
  return target.kind === "main"
    ? snapshot.state.ui.timeline.room_id === target.room_id
    : snapshot.state.ui.thread.kind === "open" &&
        snapshot.state.ui.thread.room_id === target.room_id &&
        snapshot.state.ui.thread.root_event_id === target.root_event_id;
}

export function browserComposerForTarget(
  snapshot: DesktopSnapshot,
  target: ComposerTarget
): ComposerState | null {
  if (!browserComposerTargetIsActive(snapshot, target)) {
    return null;
  }
  return target.kind === "main"
    ? snapshot.state.ui.timeline.composer
    : snapshot.state.ui.thread.kind === "open"
      ? snapshot.state.ui.thread.composer ?? null
      : null;
}

export function browserComposerDraftTargetKey(target: ComposerTarget): string {
  return target.kind === "main"
    ? target.room_id
    : `thread\u0000${target.room_id}\u0000${target.root_event_id}`;
}

export function browserStagedUploadsForTarget(
  snapshot: DesktopSnapshot,
  target: ComposerTarget
): StagedUploadItem[] {
  if (!browserComposerTargetIsActive(snapshot, target)) {
    return [];
  }
  return target.kind === "main"
    ? snapshot.state.ui.timeline.staged_uploads
    : snapshot.state.ui.thread.kind === "open"
      ? snapshot.state.ui.thread.staged_uploads
      : [];
}

export function setBrowserStagedUploadsForTarget(
  snapshot: DesktopSnapshot,
  target: ComposerTarget,
  items: StagedUploadItem[]
): void {
  if (!browserComposerTargetIsActive(snapshot, target)) {
    return;
  }
  if (target.kind === "main") {
    snapshot.state.ui.timeline.staged_uploads = items;
  } else if (snapshot.state.ui.thread.kind === "open") {
    snapshot.state.ui.thread.staged_uploads = items;
  }
}

export function browserPreparedUploadKey(
  target: ComposerTarget,
  stagedId: string,
  variantId: string
): string {
  return `${target.kind}:${target.room_id}:${target.kind === "thread" ? target.root_event_id : ""}:${stagedId}:${variantId}`;
}

export function browserPreparedUploadItem(
  target: ComposerTarget,
  item: StageUploadBytesRequestItem,
  index: number
): StagedUploadItem {
  const mime = item.mimeType.trim() || "application/octet-stream";
  const imageFormat = browserImageFormat(mime);
  const original: PreparedUploadVariant = {
    variant_id: "original-keep",
    resize: "original",
    format_choice: "keep",
    filename: item.filename || "attachment",
    mime_type: mime,
    byte_count: item.bytes.length,
    width: imageFormat ? 64 : null,
    height: imageFormat ? 48 : null,
    format: imageFormat ?? "original",
    savings_percent: 0,
    metadata_stripped: false,
    thumbnail_refreshed: false
  };
  const variants = [original];
  if (imageFormat === "png") {
    variants.push(
      browserSyntheticVariant(item, "resized-png", "png", "image/png", 25),
      browserSyntheticVariant(item, "webp", "webp", "image/webp", 35)
    );
  } else if (imageFormat === "jpeg") {
    variants.push(
      browserSyntheticVariant(item, "resized-jpeg", "jpeg", "image/jpeg", 25),
      browserSyntheticVariant(item, "webp", "webp", "image/webp", 35)
    );
  } else if (imageFormat === "webp") {
    variants.push(browserSyntheticVariant(item, "resized-webp", "webp", "image/webp", 25));
  }
  // Staging always asks and starts at the untouched output (#305).
  const selected = original;
  return {
    staged_id: item.stagedId,
    room_id: target.room_id,
    position: item.position || index + 1,
    filename: selected.filename,
    mime_type: selected.mime_type,
    byte_count: selected.byte_count,
    kind: imageFormat
      ? { kind: "image", width: selected.width, height: selected.height }
      : { kind: "file" },
    caption: null,
    compression_choice: imageFormat ? { kind: "original" } : { kind: "notApplicable" },
    preparation: {
      kind: "ready",
      variants,
      selected: { resize: "original", format: "keep" },
      pending: null,
      generation: 0
    }
  };
}

function browserSyntheticVariant(
  item: StageUploadBytesRequestItem,
  variantId: string,
  format: "png" | "jpeg" | "webp",
  mimeType: string,
  savingsPercent: number
): PreparedUploadVariant {
  const extension = format === "jpeg" ? "jpg" : format;
  const stem = item.filename.replace(/\.[^.]*$/, "") || "attachment";
  return {
    variant_id: variantId,
    // The browser fake mirrors the axis identity: synthetic variants stand in
    // for the halved output in each format.
    resize: "half",
    format_choice: format,
    filename: `${stem}.${extension}`,
    mime_type: mimeType,
    byte_count: Math.max(1, Math.floor(item.bytes.length * (1 - savingsPercent / 100))),
    width: 48,
    height: 36,
    format,
    savings_percent: savingsPercent,
    metadata_stripped: true,
    thumbnail_refreshed: true
  };
}

function browserImageFormat(mime: string): "png" | "jpeg" | "webp" | null {
  if (mime === "image/png") return "png";
  if (mime === "image/jpeg") return "jpeg";
  if (mime === "image/webp") return "webp";
  return null;
}
