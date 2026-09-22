import { expect, test, type Page } from "@playwright/test";
import type { DesktopSnapshot, RoomListItem } from "../src/domain/types";
import { t } from "../src/i18n/messages";
import { pushDelta, pushSnapshot } from "./support/stateUpdates";

// #955: the Low priority section, its scoped collapse preference, and the rule
// that only a Rust snapshot moves a conversation between sections.
interface Harness {
  currentSnapshot(): DesktopSnapshot;
  clearInvocations(): void;
  invocationsOf(command: string): unknown[];
  setCommandResponse(command: string, response: unknown): void;
}

const LABELS = {
  rooms: t("roomList.categoryRooms"),
  dms: t("roomList.categoryDms"),
  lowPriority: t("workspace.lowPriority")
};

function row(room_id: string, display_name: string, unread: number): RoomListItem {
  return {
    room_id,
    display_name,
    avatar: null,
    tags: { favourite: null, low_priority: null },
    unread_count: unread,
    notification_count: unread,
    display_count: unread,
    highlight_count: 0,
    has_unread_content: unread > 0,
    is_attention_highlighted: false,
    has_unread_mention: false,
    is_muted: false
  };
}

function lowPriority(item: RoomListItem): RoomListItem {
  return { ...item, tags: { favourite: null, low_priority: { order: null } } };
}

const PLAIN = row("!plain:example.invalid", "Plain Room", 5);
const QUIET = row("!quiet:example.invalid", "Quiet Room", 8);
const PERSON = row("!person:example.invalid", "Person", 3);

async function openHarness(page: Page): Promise<DesktopSnapshot> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("complementary", { name: t("workspace.rooms") })).toBeVisible();
  return page.evaluate(() => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    harness.setCommandResponse("close_search", { protocolVersion: 1, publishedGeneration: 0 });
    return harness.currentSnapshot();
  });
}

function sidebarWith(
  base: DesktopSnapshot,
  rooms: RoomListItem[],
  people: RoomListItem[],
  low: RoomListItem[],
  counts: { space: number; dm: number },
  lowPriorityCollapsed = false
): DesktopSnapshot["sidebar"] {
  return {
    ...base.sidebar,
    active_space_id: null,
    account_home: { ...base.sidebar.account_home, is_active: true },
    space_rail: base.sidebar.space_rail.map((space) => ({ ...space, is_active: false })),
    space_rooms: [...rooms, ...low.filter((item) => !people.includes(item))],
    global_dms: people,
    space_unread_count: counts.space,
    dm_unread_count: counts.dm,
    space_highlight_count: 0,
    dm_highlight_count: 0,
    rooms_collapsed: false,
    dms_collapsed: false,
    low_priority_collapsed: lowPriorityCollapsed,
    sections: { favourites: [], not_joined: [], rooms, people, low_priority: low }
  };
}

test("a tag change moves a conversation only when the Rust snapshot says so", async ({ page }) => {
  const base = await openHarness(page);
  await pushSnapshot(page, {
    ...base,
    sidebar: sidebarWith(base, [PLAIN, QUIET], [PERSON], [], { space: 13, dm: 3 }),
    state: {
      ...base.state,
      ui: {
        ...base.state.ui,
        navigation: { ...base.state.ui.navigation, active_space_id: null }
      }
    }
  });

  const rooms = page.getByRole("region", { name: LABELS.rooms, exact: true });
  await expect(rooms.locator(".room-name")).toHaveText(["Plain Room", "Quiet Room"]);
  await expect(page.getByRole("region", { name: LABELS.lowPriority, exact: true })).toHaveCount(0);
  await expect(rooms.locator(".section-unread-count")).toHaveText("13");

  // Sending the typed tag command must not relocate the row on its own.
  await page.evaluate(() => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    harness.clearInvocations();
  });
  await rooms.getByRole("button", { name: "Quiet Room" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: t("context.addToLowPriority"), exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as unknown as { __harness: Harness }).__harness.invocationsOf("set_room_tag")
            .length
      )
    )
    .toBe(1);
  await expect(rooms.locator(".room-name")).toHaveText(["Plain Room", "Quiet Room"]);
  await expect(page.getByRole("region", { name: LABELS.lowPriority, exact: true })).toHaveCount(0);

  // The Rust snapshot is what moves it, exactly once, and drops its unread
  // contribution from the Rooms heading while the row keeps its own count.
  await pushDelta(page, {
    sidebar: sidebarWith(base, [PLAIN], [PERSON], [lowPriority(QUIET)], { space: 5, dm: 3 })
  });
  const low = page.getByRole("region", { name: LABELS.lowPriority, exact: true });
  await expect(rooms.locator(".room-name")).toHaveText(["Plain Room"]);
  await expect(low.locator(".room-name")).toHaveText(["Quiet Room"]);
  await expect(rooms.locator(".section-unread-count")).toHaveText("5");
  await expect(low.locator(".section-unread-count")).toHaveCount(0);
  await expect(low.getByRole("button", { name: "Quiet Room" }).locator(".room-count")).toHaveText(
    "8"
  );

  // Search still reaches the low-priority row, so no global "no matches".
  const filter = page.getByRole("searchbox", { name: /filter/i });
  await filter.fill("Quiet");
  await expect(low.locator(".room-name")).toHaveText(["Quiet Room"]);
  await expect(page.locator(".room-list-no-matches")).toHaveCount(0);
  await filter.fill("no-such-conversation");
  await expect(page.locator(".room-list-no-matches")).toHaveCount(1);
  await filter.fill("");

  // Removing the tag restores the section and the heading total.
  await pushDelta(page, {
    sidebar: sidebarWith(base, [PLAIN, QUIET], [PERSON], [], { space: 13, dm: 3 })
  });
  await expect(rooms.locator(".room-name")).toHaveText(["Plain Room", "Quiet Room"]);
  await expect(page.getByRole("region", { name: LABELS.lowPriority, exact: true })).toHaveCount(0);
  await expect(rooms.locator(".section-unread-count")).toHaveText("13");
});

test("Low priority collapse submits a typed scoped patch and renders the Rust result", async ({
  page
}) => {
  const base = await openHarness(page);
  await pushSnapshot(page, {
    ...base,
    sidebar: sidebarWith(base, [PLAIN], [PERSON], [lowPriority(QUIET)], { space: 5, dm: 3 }),
    state: {
      ...base.state,
      ui: {
        ...base.state.ui,
        navigation: { ...base.state.ui.navigation, active_space_id: null }
      }
    }
  });

  const low = page.getByRole("region", { name: LABELS.lowPriority, exact: true });
  const toggle = low.getByRole("button", { name: LABELS.lowPriority, exact: true });
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.clearInvocations()
  );

  await toggle.click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as unknown as { __harness: Harness }).__harness.invocationsOf(
            "update_settings"
          )[0]
      )
    )
    .toMatchObject({
      command: "update_settings",
      args: {
        patch: { sidebar_section: { scope: "__home__", section: "lowPriority", collapsed: true } }
      }
    });
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(low.locator(".room-name")).toHaveCount(0);
  // The section header stays visible while collapsed.
  await expect(low).toBeVisible();
});
