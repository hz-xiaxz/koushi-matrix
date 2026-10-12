// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "./Shell";
import { readyDesktopSnapshotFixture } from "../test/desktopApiFixture";
import { sidebarSectionBoundsForFake } from "../test/sidebarSectionBoundsFixture";
import type { DesktopSnapshot, RoomListItem } from "../domain/types";

/**
 * #1363/#1366: the sidebar row bound and its expand control.
 *
 * The bound itself is Rust state; these tests drive the renderer with
 * Rust-authored bound values and prove React only slices the section, keeps the
 * whole-section header count, and dispatches the typed `expanded` preference.
 */

function sectionRoom(room_id: string, display_name: string): RoomListItem {
  return {
    room_id,
    display_name,
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
    sectionRoom(`!${prefix}-${index}:example.invalid`, `${prefix} ${index}`)
  );
}

function sidebarProps() {
  return {
    activeRoomId: null,
    activeView: "activity" as const,
    onCreateRoom: vi.fn(),
    onNewDm: vi.fn(),
    onOpenContextMenu: vi.fn(),
    onOpenActivity: vi.fn(),
    onOpenExplore: vi.fn(),
    onOpenInvites: vi.fn(),
    onOpenSpaceInfo: vi.fn(),
    onSelectRoom: vi.fn(),
    onUpdateSettings: vi.fn()
  };
}

/**
 * A snapshot whose Rooms/People/Low priority sections hold `count` rows each.
 * The bounds come from the generated Rust fixture, keyed by the section length
 * and the fake's own `expanded` preference.
 */
function boundedSnapshot(
  count: number,
  options: { expanded?: boolean; dmExpanded?: boolean } = {}
): DesktopSnapshot {
  const snapshot = readyDesktopSnapshotFixture();
  const snapshotRooms = rooms(count, "room");
  const people = rooms(count, "dm");
  const lowPriority = rooms(count, "low");
  snapshot.state.ui.navigation.active_space_id = null;
  snapshot.state.ui.navigation.active_room_id = null;
  snapshot.sidebar.active_space_id = null;
  snapshot.sidebar.account_home.is_active = true;
  snapshot.sidebar.space_rail = [];
  snapshot.sidebar.space_rooms = snapshotRooms;
  snapshot.sidebar.global_dms = people;
  // A non-zero whole-section unread total, so a truncated badge would show.
  snapshot.sidebar.space_unread_count = count;
  snapshot.sidebar.dm_unread_count = count;
  snapshot.sidebar.sections = {
    favourites: [],
    rooms: snapshotRooms,
    people,
    low_priority: lowPriority,
    not_joined: []
  };
  snapshot.sidebar.section_bounds = sidebarSectionBoundsForFake({
    sections: { rooms: snapshotRooms, people, low_priority: lowPriority },
    expanded: {
      rooms: options.expanded ?? false,
      dms: options.dmExpanded ?? false,
      lowPriority: false
    },
    activeRoomId: null
  });
  return snapshot;
}

function sectionRows(sectionId: string): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>(`[data-room-section="${sectionId}"] .room-item`)
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("sidebar section bound", () => {
  it("renders the bounded rows and a control naming the hidden count", () => {
    const snapshot = boundedSnapshot(8);
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    expect(sectionRows("rooms")).toHaveLength(5);
    expect(
      screen.getByRole("button", { name: "Show 3 more in Rooms" })
    ).toHaveProperty("ariaExpanded", "false");
    const control = screen.getByRole("button", { name: "Show 3 more in Rooms" });
    expect(control.getAttribute("aria-controls")).toBe("rooms-room-list");
    expect(control.textContent).toContain("Show 3 more");
  });

  it("keeps the section header count at the whole section size", () => {
    const snapshot = boundedSnapshot(7);
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    // Low priority has no Rust unread total, so its header renders the count:
    // the section's seven rows, never the five the bound shows.
    const lowPriority = document.querySelector('[data-room-section="low-priority"]');
    expect(lowPriority?.querySelector(".section-count")?.textContent).toBe("7");
    expect(sectionRows("low-priority")).toHaveLength(5);
    // Rooms and DMs replace the count with the Rust-owned whole-section unread
    // badge, so the bound never leaks a truncated total into the header: the
    // badge reports all seven rooms while five rows render.
    const rooms = document.querySelector('[data-room-section="rooms"]');
    expect(rooms?.querySelector(".section-count")).toBeNull();
    expect(rooms?.querySelector(".section-unread-count")?.textContent).toBe("7");
    expect(sectionRows("rooms")).toHaveLength(5);
  });

  it("dispatches the persisted bound preference when the control is pressed", () => {
    const props = sidebarProps();
    render(<Sidebar snapshot={boundedSnapshot(8)} {...props} />);

    fireEvent.click(screen.getByRole("button", { name: "Show 3 more in Rooms" }));
    expect(props.onUpdateSettings).toHaveBeenCalledWith({
      sidebar_section: { scope: "__home__", section: "rooms", expanded: true }
    });
  });

  it("renders every row and offers to bound the section again when expanded", () => {
    const props = sidebarProps();
    render(<Sidebar snapshot={boundedSnapshot(8, { expanded: true })} {...props} />);

    expect(sectionRows("rooms")).toHaveLength(8);
    const control = screen.getByRole("button", { name: "Show fewer in Rooms" });
    expect(control).toHaveProperty("ariaExpanded", "true");

    fireEvent.click(control);
    expect(props.onUpdateSettings).toHaveBeenCalledWith({
      sidebar_section: { scope: "__home__", section: "rooms", expanded: false }
    });
  });

  it("shows no control for a section at or below the bound", () => {
    render(<Sidebar snapshot={boundedSnapshot(5)} {...sidebarProps()} />);

    expect(sectionRows("rooms")).toHaveLength(5);
    expect(screen.queryByRole("button", { name: /more in Rooms/u })).toBeNull();
  });

  it("applies the bound to DMs and Low priority independently", () => {
    render(<Sidebar snapshot={boundedSnapshot(7, { expanded: true })} {...sidebarProps()} />);

    expect(sectionRows("dms")).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Show 2 more in DMs" })).toBeTruthy();
    expect(sectionRows("low-priority")).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Show 2 more in Low priority" })).toBeTruthy();
  });

  it("shows every matching row and no control while the filter is active", () => {
    const snapshot = boundedSnapshot(8);
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    fireEvent.change(screen.getByLabelText("Filter conversations"), {
      target: { value: "room" }
    });

    // The filter is presentation state Rust cannot see, so it bypasses the
    // bound rather than hiding a match behind the control.
    expect(sectionRows("rooms")).toHaveLength(8);
    expect(screen.queryByRole("button", { name: /more in Rooms/u })).toBeNull();
  });

  it("ignores a bound that describes a different list", () => {
    // A stale fake snapshot can carry a bound projected for another section
    // length. The renderer then shows every row: it may never hide a
    // conversation because its input disagrees with Rust's.
    const snapshot = boundedSnapshot(8);
    const stale = snapshot.sidebar.section_bounds?.rooms;
    snapshot.sidebar.section_bounds!.rooms = { ...stale!, total_count: 99 };
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    expect(sectionRows("rooms")).toHaveLength(8);
    expect(
      document.querySelector('[data-room-section="rooms"] .room-section-bound')
    ).toBeNull();
    // The other sections keep their own, matching bounds.
    expect(sectionRows("dms")).toHaveLength(5);
  });

  it("renders no rows or control for a collapsed section, whatever the bound", () => {
    // Collapse and bound are independent dimensions; collapsed wins for
    // rendering and the bound control never appears in a collapsed section.
    for (const expanded of [false, true]) {
      cleanup();
      const snapshot = boundedSnapshot(8, { expanded });
      snapshot.sidebar.rooms_collapsed = true;
      render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

      expect(sectionRows("rooms")).toHaveLength(0);
      expect(document.querySelector('[data-room-section="rooms"] .room-section-bound')).toBeNull();
    }
  });

  it("slices with Rust's visible_count instead of deriving it", () => {
    // Every generated case satisfies `expanded ? total : min(total, 5)`, so a
    // renderer that re-derived the rule would pass those. This bound disagrees
    // with the rule on purpose: only reading `visible_count` renders two rows.
    const snapshot = boundedSnapshot(8, { expanded: true });
    snapshot.sidebar.section_bounds!.rooms = {
      ...snapshot.sidebar.section_bounds!.rooms!,
      visible_count: 2,
      hidden_count: 6,
      expanded: true,
      expanded_by_active_room: false,
      shows_expand_control: true
    };
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    expect(sectionRows("rooms")).toHaveLength(2);
    const control = document.querySelector<HTMLElement>(
      '[data-room-section="rooms"] .room-section-bound'
    );
    expect(control).not.toBeNull();
    // `aria-controls` must point at the section it bounds, not at a missing id.
    expect(document.getElementById(control!.getAttribute("aria-controls")!)).not.toBeNull();
    expect(control).toHaveProperty("ariaExpanded", "true");
  });

  it("renders every row without a control when the open conversation holds the section open", () => {
    const snapshot = boundedSnapshot(8);
    // Rust clears the control while the open conversation sits past the bound,
    // because bounding the section again could not change what is visible.
    snapshot.sidebar.section_bounds = {
      ...snapshot.sidebar.section_bounds,
      rooms: {
        ...snapshot.sidebar.section_bounds!.rooms!,
        visible_count: 8,
        hidden_count: 0,
        expanded: true,
        expanded_by_active_room: true,
        shows_expand_control: false
      }
    };
    render(<Sidebar snapshot={snapshot} {...sidebarProps()} />);

    expect(sectionRows("rooms")).toHaveLength(8);
    expect(
      document.querySelector('[data-room-section="rooms"] .room-section-bound')
    ).toBeNull();
  });
});
