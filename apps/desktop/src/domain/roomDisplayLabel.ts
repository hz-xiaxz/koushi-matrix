import { t } from "../i18n/messages";
import type {
  InvitePreview,
  RoomListItem,
  RoomNamePlaceholder,
  RoomSummary,
  SpaceChildSummary
} from "./types";

/**
 * Longest room label a single-line surface may render.
 *
 * `m.room.name` has no length limit, and WebKit's `text-overflow: ellipsis`
 * draws nothing but the ellipsis once the text passes 10,240 characters
 * (#1404) — so anyone who can rename a room decides whether the sidebar, the
 * room header and the room pickers can show that room at all. A few hundred
 * characters is far more than any of those surfaces can display, and the full
 * name stays available in Room Info's own name field.
 */
const MAX_ROOM_LABEL_CHARS = 512;

/** Cap a caller-data room label for a single-line surface (#1404). */
export function cappedRoomLabel(label: string): string {
  const characters = [...label];
  return characters.length > MAX_ROOM_LABEL_CHARS
    ? `${characters.slice(0, MAX_ROOM_LABEL_CHARS - 1).join("")}…`
    : label;
}

function roomNamePlaceholderText(placeholder: RoomNamePlaceholder): string {
  switch (placeholder.kind) {
    case "empty":
      return t("room.namePlaceholderEmpty");
    case "emptyWas":
      return t("room.namePlaceholderEmptyWas", { previousNames: placeholder.previous_names });
  }
}

/**
 * A room's people-facing name (#1050). Rust marks the SDK's English
 * calculated empty-room name structurally; that case renders catalog text and
 * every other label stays caller data.
 */
export function roomDisplayLabel(
  room: Pick<RoomSummary, "display_label" | "display_label_placeholder">
): string {
  return room.display_label_placeholder
    ? roomNamePlaceholderText(room.display_label_placeholder)
    : cappedRoomLabel(room.display_label);
}

/** `roomDisplayLabel` for a Rust sidebar row. */
export function roomListItemLabel(
  room: Pick<RoomListItem, "display_name" | "display_name_placeholder">
): string {
  return room.display_name_placeholder
    ? roomNamePlaceholderText(room.display_name_placeholder)
    : cappedRoomLabel(room.display_name);
}

/** The localized people-facing name shown for an invite preview. */
export function invitePreviewLabel(
  invite: Pick<InvitePreview, "display_name" | "display_name_placeholder">
): string {
  return invite.display_name_placeholder
    ? roomNamePlaceholderText(invite.display_name_placeholder)
    : cappedRoomLabel(invite.display_name);
}

/** `roomDisplayLabel` for a Space child the account has not joined (#1070). */
export function spaceChildLabel(
  child: Pick<SpaceChildSummary, "display_name" | "display_name_placeholder">
): string {
  return child.display_name_placeholder
    ? roomNamePlaceholderText(child.display_name_placeholder)
    : cappedRoomLabel(child.display_name);
}
