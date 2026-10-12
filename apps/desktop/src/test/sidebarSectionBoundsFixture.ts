import type {
  RoomListItem,
  SidebarSectionBound,
  SidebarSectionBounds
} from "../domain/types";
import generated from "./sidebarSectionBounds.generated.json";

/**
 * #1363/#1366: the sidebar row bound as Rust projected it.
 *
 * `sidebarSectionBounds.generated.json` is written by
 * `crates/koushi-state/tests/sidebar_section_bound.rs` (regenerate with
 * `UPDATE_SIDEBAR_SECTION_BOUNDS_GOLDEN=1 cargo test -p koushi-state --test
 * sidebar_section_bound`) and asserted there against
 * `SidebarSectionBound::project`. Frontend fakes look their bounds up here
 * instead of recomputing the projection rule, so the browser harness cannot
 * drift from the packaged Rust state.
 */
export const SIDEBAR_SECTION_VISIBLE_LIMIT = generated.visibleLimit;

type GeneratedBoundCase = (typeof generated.cases)[number];

function boundFromCase(entry: GeneratedBoundCase): SidebarSectionBound {
  return {
    visible_limit: generated.visibleLimit,
    total_count: entry.totalCount,
    visible_count: entry.visibleCount,
    hidden_count: entry.hiddenCount,
    expanded: entry.expanded,
    expanded_by_active_room: entry.expandedByActiveRoom,
    shows_expand_control: entry.showsExpandControl
  };
}

/**
 * The bound Rust projects for one section, looked up in the generated cases.
 *
 * A shape the fixture does not cover returns `null`: callers then render every
 * row, which can never hide a conversation behind a missing bound.
 */
export function sidebarSectionBoundFromRust(
  totalCount: number,
  persistedExpanded: boolean,
  activeRoomIndex: number | null
): SidebarSectionBound | null {
  const entry = generated.cases.find(
    (candidate) =>
      candidate.totalCount === totalCount &&
      candidate.persistedExpanded === persistedExpanded &&
      candidate.activeRoomIndex === activeRoomIndex
  );
  return entry ? boundFromCase(entry) : null;
}

function boundForSection(
  rooms: Pick<RoomListItem, "room_id">[],
  expanded: boolean,
  activeRoomId: string | null
): SidebarSectionBound | null {
  const activeRoomIndex =
    activeRoomId === null ? null : rooms.findIndex((room) => room.room_id === activeRoomId);
  return sidebarSectionBoundFromRust(
    rooms.length,
    expanded,
    activeRoomIndex === -1 ? null : activeRoomIndex
  );
}

/**
 * Bounds for a fake snapshot's sections.
 *
 * `expanded` comes from the fake's stored settings and the active room from the
 * fake's navigation; the numbers themselves come from the generated fixture.
 */
export function sidebarSectionBoundsForFake(input: {
  sections: {
    rooms: Pick<RoomListItem, "room_id">[];
    people: Pick<RoomListItem, "room_id">[];
    low_priority: Pick<RoomListItem, "room_id">[];
  };
  expanded: { rooms: boolean; dms: boolean; lowPriority: boolean };
  activeRoomId: string | null;
}): SidebarSectionBounds {
  return {
    rooms: boundForSection(
      input.sections.rooms,
      input.expanded.rooms,
      input.activeRoomId
    ),
    dms: boundForSection(input.sections.people, input.expanded.dms, input.activeRoomId),
    low_priority: boundForSection(
      input.sections.low_priority,
      input.expanded.lowPriority,
      input.activeRoomId
    )
  };
}
