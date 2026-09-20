/**
 * Headless spec: a Space shows the rooms the account has not joined (#961).
 *
 * The not-joined lane sits below the joined sections, each row says which
 * relationship it is in, and activating one asks Rust to join the room rather
 * than selecting a room the account cannot read.
 */

import { expect, test, type Page } from "@playwright/test";
import type { StateUpdateEnvelope } from "../src/domain/coreEvents";
import type { DesktopSnapshot, RoomListItem } from "../src/domain/types";
import { t } from "../src/i18n/messages";

interface Harness {
  currentSnapshot(): DesktopSnapshot;
  pushStateUpdate(update: StateUpdateEnvelope): void;
  clearInvocations(): void;
  invocationsOf(command: string): unknown[];
  setCommandResponse(command: string, response: unknown): void;
}

function row(
  room_id: string,
  display_name: string,
  membership: RoomListItem["membership"] = "joined"
): RoomListItem {
  return {
    room_id,
    display_name,
    membership,
    can_join: membership !== "joined",
    avatar: null,
    tags: { favourite: null, low_priority: null },
    unread_count: 0,
    notification_count: 0,
    display_count: 0,
    highlight_count: 0,
    has_unread_content: false,
    is_attention_highlighted: false,
    has_unread_mention: false,
    is_muted: false
  };
}

const JOINED = row("!joined:example.invalid", "Joined Room");
const LOW = row("!low:example.invalid", "Low Room");
const OPEN = row("!open:example.invalid", "Open Room", "not_joined");
const INVITED = row("!invited:example.invalid", "Invited Room", "invited");

async function openHarness(page: Page): Promise<DesktopSnapshot> {
  // Sidebar section collapse is a persisted preference, so a spec that ran
  // earlier in the same worker could otherwise leave the Not joined section
  // collapsed and hide every row this spec asserts on.
  await page.addInitScript(() => {
    try {
      window.localStorage.clear();
    } catch {
      // A blocked storage accessor is not this spec's concern.
    }
  });
  await page.goto("/appHarness.html");
  await expect(page.getByRole("complementary", { name: t("workspace.rooms") })).toBeVisible();
  return page.evaluate(() => {
    const harness = (window as unknown as { __harness: Harness }).__harness;
    harness.setCommandResponse("join_room", { protocolVersion: 1, publishedGeneration: 0 });
    harness.clearInvocations();
    return harness.currentSnapshot();
  });
}

test("a Space lists the rooms it contains that the account has not joined", async ({ page }) => {
  const base = await openHarness(page);
  const generation = (base.state_generation ?? 0) + 1;
  await page.evaluate(
    ({ envelope }) => {
      (window as unknown as { __harness: Harness }).__harness.pushStateUpdate(
        envelope as StateUpdateEnvelope
      );
    },
    {
      envelope: {
        protocol_version: 1,
        kind: "delta",
        generation,
        changed: {
          sidebar: {
            ...base.sidebar,
            space_rooms: [JOINED],
            sections: {
              favourites: [],
              rooms: [JOINED],
              people: [],
              low_priority: [LOW],
              not_joined: [OPEN, INVITED]
            }
          }
        }
      }
    }
  );

  const open = page.getByRole("button", { name: /Open Room/ });
  const invited = page.getByRole("button", { name: /Invited Room/ });
  await expect(open).toBeVisible();
  await expect(invited).toBeVisible();

  // Each row says what it is, so an invitation is not mistaken for a room that
  // is merely open to join.
  await expect(open).toContainText(t("roomList.membershipNotJoined"));
  await expect(invited).toContainText(t("roomList.membershipInvited"));

  // The lane sits below the joined sections.
  const order = await page.evaluate(() => {
    const labels = Array.from(document.querySelectorAll("button .room-name")).map(
      (node) => node.textContent ?? ""
    );
    return labels;
  });
  expect(order.indexOf("Open Room")).toBeGreaterThan(order.indexOf("Low Room"));
  expect(order.indexOf("Low Room")).toBeGreaterThan(order.indexOf("Joined Room"));

  await open.click();
  const joins = await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.invocationsOf("join_room")
  );
  expect(joins).toHaveLength(1);
  expect(JSON.stringify(joins[0])).toContain("!open:example.invalid");

  // An invitation is answered in the invites view, which owns accept and
  // decline; a click here never joins around it.
  await invited.click();
  const joinsAfterInvite = await page.evaluate(() =>
    (window as unknown as { __harness: Harness }).__harness.invocationsOf("join_room")
  );
  expect(joinsAfterInvite).toHaveLength(1);
});
