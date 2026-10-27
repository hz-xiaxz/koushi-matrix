/**
 * Headless spec: Activity rows are a visible avatar surface. A row whose
 * sender avatar is still `notRequested` must submit the same typed thumbnail
 * demand as sidebar/member surfaces instead of waiting for another pane to
 * download it.
 */

import { expect, test } from "@playwright/test";

import { t } from "../src/i18n/messages";
import { gotoReadyShell, invocationCount } from "./support/basicOperations";

const ROOM_ID = "!activity-avatar-room:example.invalid";
const EVENT_ID = "$activity-avatar-event:example.invalid";
const MXC_URI = "mxc://example.invalid/activity-avatar";

test("Activity requests thumbnails for visible sender avatars", async ({ page }) => {
  await gotoReadyShell(page);
  const before = await invocationCount(page, "download_avatar_thumbnail");

  await page.evaluate(({ roomId, eventId, mxcUri }) => {
    const snapshot = window.__harness.currentSnapshot();
    const opened = {
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          activity: {
            kind: "open" as const,
            active_tab: "recent" as const,
            recent: {
              rows: [{
                kind: "event" as const,
                room_id: roomId,
                event_id: eventId,
                thread_root_event_id: null,
                sender_id: "@member-1:example.invalid",
                room_label: "Synthetic activity room",
                sender_label: "Member 1",
                sender_avatar: { mxc_uri: mxcUri, thumbnail: { kind: "notRequested" as const } },
                preview: "Synthetic activity preview",
                timestamp_ms: 1_800_000_000_000,
                unread: false,
                highlight: false,
                context_label: "Room"
              }],
              next_batch: null,
              resolution: { kind: "idle" as const }
            },
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
  }, { roomId: ROOM_ID, eventId: EVENT_ID, mxcUri: MXC_URI });

  await page
    .getByRole("navigation", { name: t("workspace.workspaces") })
    .getByRole("button", { name: /^Home/ })
    .click();

  const pane = page.getByRole("main", { name: t("workspace.activity") });
  await expect(pane).toBeVisible();
  await expect(pane.getByRole("listitem")).toHaveCount(1);

  await expect
    .poll(() => invocationCount(page, "download_avatar_thumbnail"))
    .toBeGreaterThan(before);
  await expect
    .poll(() =>
      page.evaluate(
        (mxcUri) =>
          window.__harness
            .invocationsOf("download_avatar_thumbnail")
            .some((invocation) => invocation.args.mxcUri === mxcUri),
        MXC_URI
      )
    )
    .toBe(true);
});
