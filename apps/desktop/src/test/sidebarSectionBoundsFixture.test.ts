import { describe, expect, test } from "vitest";

import type { DesktopSnapshot } from "../domain/types";
import { readyDesktopSnapshotFixture } from "./desktopApiFixture";
import { TauriIpcMock } from "./tauriIpcMock";
import {
  SIDEBAR_SECTION_VISIBLE_LIMIT,
  sidebarSectionBoundFromRust,
  sidebarSectionBoundsForFake
} from "./sidebarSectionBoundsFixture";
import generated from "./sidebarSectionBounds.generated.json";

/**
 * #1363/#1366: the frontend fakes must project the sidebar bound exactly as the
 * packaged Rust state does.
 *
 * `sidebarSectionBounds.generated.json` is written and asserted against
 * `koushi_state::SidebarSectionBound::project` by
 * `crates/koushi-state/tests/sidebar_section_bound.rs`. These tests prove the
 * fakes only look those values up: every generated case resolves, the lookup
 * cannot confuse two cases, and the mock and fixture snapshots carry the
 * generated numbers instead of their own arithmetic.
 */
describe("sidebar section bounds fixture", () => {
  test("every Rust-generated case resolves to its own bound", () => {
    for (const entry of generated.cases) {
      const bound = sidebarSectionBoundFromRust(
        entry.totalCount,
        entry.persistedExpanded,
        entry.activeRoomIndex
      );
      expect(bound, `case ${JSON.stringify(entry)} must be covered`).not.toBeNull();
      expect(bound).toEqual({
        visible_limit: SIDEBAR_SECTION_VISIBLE_LIMIT,
        total_count: entry.totalCount,
        visible_count: entry.visibleCount,
        hidden_count: entry.hiddenCount,
        expanded: entry.expanded,
        expanded_by_active_room: entry.expandedByActiveRoom,
        shows_expand_control: entry.showsExpandControl
      });
    }
  });

  test("case keys are unique, so no lookup can pick another shape", () => {
    const keys = generated.cases.map(
      (entry) => `${entry.totalCount}/${entry.persistedExpanded}/${entry.activeRoomIndex}`
    );
    expect(new Set(keys).size).toBe(keys.length);
  });

  test("a shape the fixture does not cover renders every row", () => {
    // Conservative fallback: a missing bound never hides a conversation.
    expect(sidebarSectionBoundFromRust(9, false, null)).toBeNull();
    expect(sidebarSectionBoundFromRust(0, false, 3)).toBeNull();
  });

  test("an open conversation past the bound keeps its row without a control", () => {
    // Persisted `false`, active row index 5 of 6: Rust forces the section open
    // and clears the control, because bounding it could not hide the open row.
    const bound = sidebarSectionBoundFromRust(6, false, 5);
    expect(bound).toMatchObject({
      expanded: true,
      expanded_by_active_room: true,
      shows_expand_control: false,
      visible_count: 6
    });
  });

  test("the generated bound never shows more than the limit unless it is expanded", () => {
    for (const entry of generated.cases) {
      if (entry.expanded) {
        expect(entry.visibleCount).toBe(entry.totalCount);
        expect(entry.hiddenCount).toBe(0);
      } else {
        expect(entry.visibleCount).toBe(
          Math.min(entry.totalCount, SIDEBAR_SECTION_VISIBLE_LIMIT)
        );
        expect(entry.hiddenCount).toBe(entry.totalCount - entry.visibleCount);
      }
    }
  });

  // This is a shape-consistency check between the two fakes, not the wire
  // guard: the Rust agreement is proved by the Rust test that writes and
  // asserts `sidebarSectionBounds.generated.json`, which the lookup above
  // replays case by case.
  test("the default mock and the ready fixture attach the same looked-up bounds", () => {
    const snapshot = readyDesktopSnapshotFixture();
    const expected = sidebarSectionBoundsForFake({
      sections: { rooms: [], people: [], low_priority: [] },
      expanded: { rooms: false, dms: false, lowPriority: false },
      activeRoomId: null
    });
    const mock = new TauriIpcMock();
    return mock.invoke<DesktopSnapshot>("get_snapshot").then((defaults) => {
      expect(defaults.sidebar.section_bounds).toEqual(expected);
      expect(snapshot.sidebar.section_bounds).toEqual(
        sidebarSectionBoundsForFake({
          sections: {
            rooms: snapshot.sidebar.sections.rooms,
            people: snapshot.sidebar.sections.people,
            low_priority: snapshot.sidebar.sections.low_priority
          },
          expanded: { rooms: false, dms: false, lowPriority: false },
          activeRoomId: "!room-alpha:example.invalid"
        })
      );
    });
  });
});
