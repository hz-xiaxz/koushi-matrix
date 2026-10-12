// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { readyDesktopSnapshotFixture } from "./desktopApiFixture";
import { sidebarSectionBoundsForFake } from "./sidebarSectionBoundsFixture";
import { withSectionBounds } from "./withSectionBounds";
import type { DesktopSnapshot, RoomListItem } from "../domain/types";

/**
 * #1363/#1366: the browser fake attaches the Rust-projected bound, and a
 * partial snapshot without a sidebar projection passes through untouched.
 *
 * The signed-out auth spec installs a snapshot whose `sidebar` literal carries
 * no `sections`; an unguarded helper threw there and took the whole auth screen
 * down, which the browser-headless tier caught.
 */

function sectionRoom(roomId: string): RoomListItem {
  return {
    room_id: roomId,
    display_name: roomId,
    avatar: null,
    tags: { favourite: null, low_priority: null },
    unread_count: 0,
    highlight_count: 0,
    notification_count: 0,
    display_count: 0,
    has_unread_content: false,
    is_attention_highlighted: false,
    has_unread_mention: false,
    is_muted: false
  };
}

function rooms(count: number, prefix: string): RoomListItem[] {
  return Array.from({ length: count }, (_, index) =>
    sectionRoom(`!${prefix}-${index}:example.invalid`)
  );
}

describe("withSectionBounds", () => {
  it("leaves a partial signed-out snapshot without a sidebar projection alone", () => {
    const snapshot = readyDesktopSnapshotFixture();
    // The shape `e2e/profile-settings-session.spec.ts` installs for fresh auth.
    const partial = {
      ...snapshot,
      sidebar: {
        active_space_id: null,
        space_rooms: [],
        global_dms: []
      }
    } as unknown as DesktopSnapshot;

    expect(withSectionBounds(partial)).toBe(partial);
  });

  it("attaches the generated bounds for the fake's own sections", () => {
    const snapshot = readyDesktopSnapshotFixture();
    snapshot.state.ui.navigation.active_space_id = null;
    snapshot.state.ui.navigation.active_room_id = null;
    snapshot.sidebar.sections.rooms = rooms(8, "room");
    snapshot.sidebar.sections.people = rooms(6, "dm");
    snapshot.sidebar.sections.low_priority = [];

    const bounded = withSectionBounds(snapshot);

    expect(bounded.sidebar.section_bounds).toEqual(
      sidebarSectionBoundsForFake({
        sections: {
          rooms: snapshot.sidebar.sections.rooms,
          people: snapshot.sidebar.sections.people,
          low_priority: []
        },
        expanded: { rooms: false, dms: false, lowPriority: false },
        activeRoomId: null
      })
    );
    expect(bounded.sidebar.section_bounds?.rooms?.visible_count).toBe(5);
    expect(bounded.sidebar.section_bounds?.dms?.hidden_count).toBe(1);
    expect(bounded.sidebar.section_bounds?.rooms?.shows_expand_control).toBe(true);
  });

  it("reads the scope's stored expansion and the open conversation", () => {
    const snapshot = readyDesktopSnapshotFixture();
    snapshot.state.ui.navigation.active_space_id = null;
    snapshot.state.ui.navigation.active_room_id = "!room-6:example.invalid";
    snapshot.sidebar.sections.rooms = rooms(8, "room");
    snapshot.sidebar.sections.people = [];
    snapshot.sidebar.sections.low_priority = [];
    snapshot.state.domain.settings.values.sidebar.scope_preferences = {
      __home__: {
        rooms: {
          collapsed: false,
          sort: { kind: "recentFirst" },
          expanded: true
        },
        dms: { collapsed: false, sort: { kind: "recentFirst" }, expanded: false }
      }
    };

    const bounded = withSectionBounds(snapshot);

    // The persisted bound wins, so every row shows and the open row is not the
    // reason: the control stays (as "Show fewer") because the bound is the
    // user's own choice.
    expect(bounded.sidebar.section_bounds?.rooms).toMatchObject({
      expanded: true,
      visible_count: 8,
      hidden_count: 0,
      expanded_by_active_room: false,
      shows_expand_control: true
    });
    // A scope with no stored preference stays bounded.
    expect(bounded.sidebar.section_bounds?.dms?.expanded).toBe(false);
  });
});
