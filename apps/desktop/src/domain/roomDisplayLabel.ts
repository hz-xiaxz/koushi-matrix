import { t } from "../i18n/messages";
import type {
  InvitePreview,
  RoomListItem,
  RoomNamePlaceholder,
  RoomSummary,
  SpaceChildSummary,
  SpaceSummary
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

/**
 * The Space a room is picked under, for rows that would otherwise be identical
 * (#1403). The rule mirrors the Activity projection's
 * (`activity_row_context_label`): a DM has no containing Space, otherwise the
 * first parent Space the room declares, and failing that a Space that lists the
 * room as a child.
 */
export type SpaceContextSource = Pick<SpaceSummary, "space_id" | "display_name" | "child_room_ids">;

export function roomSpaceLabel(
  room: Pick<RoomSummary, "room_id" | "is_dm" | "parent_space_ids">,
  spaces: readonly SpaceContextSource[]
): string | null {
  if (room.is_dm) {
    return null;
  }
  const parent =
    room.parent_space_ids
      .map((spaceId) => spaces.find((space) => space.space_id === spaceId))
      .find((space) => space !== undefined) ??
    spaces.find((space) => space.child_room_ids.includes(room.room_id));
  return parent ? cappedRoomLabel(parent.display_name) : null;
}

/** `roomSpaceLabel` for every room in a list, keyed by room id. */
export function roomSpaceLabelsByRoomId(
  rooms: readonly Pick<RoomSummary, "room_id" | "is_dm" | "parent_space_ids">[],
  spaces: readonly SpaceContextSource[]
): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const room of rooms) {
    const label = roomSpaceLabel(room, spaces);
    if (label !== null) {
      labels[room.room_id] = label;
    }
  }
  return labels;
}

/**
 * The labels more than one item carries. Only a collision needs extra context;
 * a uniqueness check over the rendered labels keeps every other row unchanged.
 */
export function collidingRoomLabels(labels: readonly string[]): Set<string> {
  const seen = new Set<string>();
  const colliding = new Set<string>();
  for (const label of labels) {
    if (seen.has(label)) {
      colliding.add(label);
    } else {
      seen.add(label);
    }
  }
  return colliding;
}

/**
 * Forward-menu destinations. A room whose name collides with another
 * destination's gets its Space — or its room ID when it is in none — in front,
 * so the user can tell the rows apart before sending (#1403).
 */
export function forwardDestinationsFrom(
  rooms: readonly Pick<
    RoomSummary,
    "room_id" | "display_label" | "display_label_placeholder" | "is_dm" | "parent_space_ids"
  >[],
  spaces: readonly SpaceContextSource[]
): { room_id: string; display_name: string }[] {
  const labels = rooms.map((room) => roomDisplayLabel(room));
  const colliding = collidingRoomLabels(labels);
  return rooms.map((room, index) => {
    const label = labels[index] ?? "";
    const context = colliding.has(label)
      ? roomSpaceLabel(room, spaces) ?? room.room_id
      : null;
    return {
      room_id: room.room_id,
      display_name: context === null ? label : `${context} · ${label}`
    };
  });
}
