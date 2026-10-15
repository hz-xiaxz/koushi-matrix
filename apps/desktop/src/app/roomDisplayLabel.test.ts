import { afterEach, describe, expect, test } from "vitest";
import { setActiveLocaleProfile } from "../i18n/messages";
import { selectForwardDestinations } from "../domain/appStore";
import type { DesktopSnapshot } from "../domain/types";
import {
  cappedRoomLabel,
  collidingRoomLabels,
  forwardDestinationsFrom,
  forwardDestinationsFromSnapshot,
  roomDisplayLabel,
  roomListItemLabel,
  roomSpaceLabel
} from "./uiShared";

// #1050: Rust marks the SDK's calculated empty-room name structurally; the GUI
// renders catalog text for it and keeps every other label as caller data.
describe("roomDisplayLabel", () => {
  afterEach(() => {
    setActiveLocaleProfile("en", "none");
  });

  test("renders the empty-room placeholder through the active catalog", () => {
    const empty = { display_label: "Empty Room", display_label_placeholder: { kind: "empty" as const } };
    const emptyWas = {
      display_label: "Empty Room (was Alice)",
      display_label_placeholder: { kind: "emptyWas" as const, previous_names: "Alice" }
    };

    expect(roomDisplayLabel(empty)).toBe("Empty room");
    expect(roomDisplayLabel(emptyWas)).toBe("Empty room (was Alice)");

    setActiveLocaleProfile("ja", "none");
    expect(roomDisplayLabel(empty)).toBe("空のルーム");
    expect(roomDisplayLabel(emptyWas)).toBe("空のルーム（以前: Alice）");
  });

  test("keeps named rooms and rooms without the field as caller data", () => {
    setActiveLocaleProfile("ja", "none");
    expect(roomDisplayLabel({ display_label: "Empty Room" })).toBe("Empty Room");
    expect(roomDisplayLabel({ display_label: "研究室", display_label_placeholder: null })).toBe(
      "研究室"
    );
  });

  test("cached forward destinations follow a locale change", () => {
    const rooms = [
      { room_id: "!empty:example.invalid", display_label: "Empty Room", display_label_placeholder: { kind: "empty" } }
    ];
    const snapshot = { state: { domain: { rooms, spaces: [] } } } as unknown as DesktopSnapshot;
    const store = { snapshot };

    expect(forwardDestinationsFromSnapshot(snapshot)[0]?.display_name).toBe("Empty room");
    expect(selectForwardDestinations(store)[0]?.display_name).toBe("Empty room");
    setActiveLocaleProfile("ja", "none");
    expect(forwardDestinationsFromSnapshot(snapshot)[0]?.display_name).toBe("空のルーム");
    expect(selectForwardDestinations(store)[0]?.display_name).toBe("空のルーム");
  });

  // #1404: a room name past WebKit's 10,240-character `text-overflow: ellipsis`
  // cutoff must still render a name, so the shared helpers cap caller data.
  test("caps an over-long room label so a single-line surface still shows a name", () => {
    const longName = "long-".repeat(2100);
    expect(longName.length).toBeGreaterThan(10_240);

    const capped = roomDisplayLabel({ display_label: longName, display_label_placeholder: null });
    expect(capped).toHaveLength(512);
    expect(capped.endsWith("…")).toBe(true);
    expect(capped.trim()).not.toBe("");
    expect(roomListItemLabel({ display_name: longName })).toBe(capped);
    expect(cappedRoomLabel(longName)).toBe(capped);
    expect(cappedRoomLabel("Ops")).toBe("Ops");
  });

  test("labels sidebar rows from their mirrored placeholder", () => {
    setActiveLocaleProfile("ja", "none");
    expect(
      roomListItemLabel({ display_name: "Empty Room", display_name_placeholder: { kind: "empty" } })
    ).toBe("空のルーム");
    expect(roomListItemLabel({ display_name: "Ops" })).toBe("Ops");
  });
});

// #1403: room names are not unique, so a picker row must say which Space a room
// is in before the user sends or links the wrong one.
describe("roomSpaceLabel", () => {
  const spaces = [
    {
      space_id: "!bar:example.invalid",
      display_name: "Bar",
      child_room_ids: ["!in-bar:example.invalid"],
      parent_side_child_room_ids: []
    },
    {
      space_id: "!foobar:example.invalid",
      display_name: "foobar",
      child_room_ids: [],
      parent_side_child_room_ids: []
    }
  ];

  test("prefers a declared parent Space and falls back to a containing one", () => {
    expect(
      roomSpaceLabel(
        { room_id: "!x:example.invalid", is_dm: false, parent_space_ids: ["!foobar:example.invalid", "!bar:example.invalid"] },
        spaces
      )
    ).toBe("foobar");
    expect(
      roomSpaceLabel({ room_id: "!in-bar:example.invalid", is_dm: false, parent_space_ids: [] }, spaces)
    ).toBe("Bar");
    expect(
      roomSpaceLabel({ room_id: "!nowhere:example.invalid", is_dm: false, parent_space_ids: [] }, spaces)
    ).toBeNull();
  });

  test("a DM has no containing Space to name", () => {
    expect(
      roomSpaceLabel({ room_id: "!dm:example.invalid", is_dm: true, parent_space_ids: ["!bar:example.invalid"] }, spaces)
    ).toBeNull();
  });

  test("colliding names are the only ones that need context", () => {
    expect([...collidingRoomLabels(["a", "b", "a"])]).toEqual(["a"]);
    expect([...collidingRoomLabels(["a", "b"])]).toEqual([]);
  });

  test("forward destinations name the Space only where names collide", () => {
    const rooms = [
      { room_id: "!bar-room:example.invalid", display_label: "latticelattice", display_label_placeholder: null, is_dm: false, parent_space_ids: ["!bar:example.invalid"] },
      { room_id: "!foobar-room:example.invalid", display_label: "latticelattice", display_label_placeholder: null, is_dm: false, parent_space_ids: ["!foobar:example.invalid"] },
      { room_id: "!loose-room:example.invalid", display_label: "latticelattice", display_label_placeholder: null, is_dm: false, parent_space_ids: [] },
      { room_id: "!unique:example.invalid", display_label: "Ops", display_label_placeholder: null, is_dm: false, parent_space_ids: [] }
    ];

    expect(forwardDestinationsFrom(rooms, spaces)).toEqual([
      { room_id: "!bar-room:example.invalid", display_name: "Bar · latticelattice" },
      { room_id: "!foobar-room:example.invalid", display_name: "foobar · latticelattice" },
      { room_id: "!loose-room:example.invalid", display_name: "!loose-room:example.invalid · latticelattice" },
      { room_id: "!unique:example.invalid", display_name: "Ops" }
    ]);
  });
});
