/**
 * Headless spec (#1061): the Activity pane renders Rust-owned resolution
 * states while it is open.
 *
 * The states under e2e/fixtures/activity-resolution-states.generated.json are
 * produced by a real AppActor run
 * (`runtime::tests::activity_renderer_states` keeps them current), so this
 * spec asserts the renderer against the snapshot shapes Rust publishes rather
 * than hand-written ones. Each state arrives as a live state delta.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Page } from "@playwright/test";

import type { ActivityState } from "../src/domain/types";
import { t } from "../src/i18n/messages";
import { gotoReadyShell } from "./support/basicOperations";
import { pushDelta } from "./support/stateUpdates";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const STATES = JSON.parse(
  readFileSync(path.resolve(HERE, "fixtures/activity-resolution-states.generated.json"), "utf8")
) as Record<
  | "newUnreadDmResolving"
  | "newUnreadDmResolved"
  | "staleRowWithNewerActivityResolving"
  | "staleRowWithNewerActivityFailed",
  ActivityState
>;

const OLD_EVENT = "$activity-renderer-old:example.invalid";
const RESOLVED_EVENT = "$activity-renderer-resolved:example.invalid";

/** Open Activity on its Unread tab with nothing to resolve yet. */
async function openEmptyUnreadActivity(page: Page): Promise<void> {
  await gotoReadyShell(page);
  await page.evaluate(() => {
    const snapshot = window.__harness.currentSnapshot();
    const opened = {
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          activity: {
            kind: "open" as const,
            active_tab: "unread" as const,
            recent: { rows: [], next_batch: null, resolution: { kind: "idle" as const } },
            unread: { rows: [], next_batch: null, resolution: { kind: "idle" as const } },
            mark_read: { kind: "idle" as const }
          }
        }
      }
    };
    window.__harness.setCommandResponse("open_activity", () => {
      window.__harness.setSnapshot(opened);
      return opened;
    });
  });
  await page
    .getByRole("navigation", { name: t("workspace.workspaces") })
    .getByRole("button", { name: /^Home/ })
    .click();
  await expect(activityPane(page)).toBeVisible();
  await expect(activityPane(page).getByText(t("activity.noUnread"))).toBeVisible();
}

function activityPane(page: Page) {
  return page.getByRole("main", { name: t("workspace.activity") });
}

async function publishActivity(page: Page, activity: ActivityState): Promise<void> {
  await pushDelta(page, { state: { domain: { activity } } });
}

test("a new unread DM keeps a verified room summary through resolution", async ({
  page
}) => {
  await openEmptyUnreadActivity(page);
  const pane = activityPane(page);

  await publishActivity(page, STATES.newUnreadDmResolving);
  // The SDK count identifies the room, not a particular unread message.
  await expect(pane.getByRole("status")).toHaveCount(0);
  await expect(pane.getByRole("listitem")).toHaveCount(1);
  await expect(pane.getByText(t("activity.noUnread"))).toHaveCount(0);
  await expect(pane.getByRole("button", { name: t("activity.markAllRead") })).toHaveCount(0);

  await publishActivity(page, STATES.newUnreadDmResolved);
  await expect(pane.getByRole("status")).toHaveCount(0);
  const row = pane.getByRole("listitem");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-kind", "roomUnread");
  await expect(row).not.toHaveAttribute("data-event-id", RESOLVED_EVENT);
  await expect(row).not.toContainText("Resolved message");
});

test("an old unread row with newer room activity keeps resolving or retry visible", async ({
  page
}) => {
  await openEmptyUnreadActivity(page);
  const pane = activityPane(page);

  await publishActivity(page, STATES.staleRowWithNewerActivityResolving);
  // An old historical event must not be named as unread.
  const row = pane.getByRole("listitem");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-kind", "roomUnread");
  await expect(row).not.toHaveAttribute("data-event-id", OLD_EVENT);
  await expect(pane.getByRole("status")).toHaveCount(0);

  await publishActivity(page, STATES.staleRowWithNewerActivityFailed);
  await expect(pane.getByRole("status")).toHaveCount(0);
  await expect(pane.getByRole("alert")).toHaveCount(0);
  await expect(pane.getByRole("listitem")).not.toHaveAttribute("data-event-id", OLD_EVENT);
});
