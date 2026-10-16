import { expect, test } from "@playwright/test";

/**
 * Room-key request feedback (#460): keyboard-activating the "Request keys and
 * retry" action on an undecryptable message dispatches the typed command and
 * announces the toast in an ARIA-live status region.
 */
const USER_ID = "@harness-user:example.invalid";
const ROOM_ID = "!harness-room:example.invalid";
const UTD_EVENT_ID = "$utd-message:example.invalid";

async function seedUtdTimeline(page: import("@playwright/test").Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: "Conversation timeline" })).toBeVisible();
  await page.evaluate(
    ({ userId, roomId, eventId }) => {
      const item = {
        id: { Event: { event_id: eventId } },
        sender: userId,
        body: "Unable to decrypt message",
        timestamp_ms: 1_800_000_000_000,
        in_reply_to_event_id: null,
        thread_root: null,
        thread_summary: null,
        can_react: false,
        is_redacted: false,
        is_hidden: false,
        can_redact: false,
        is_edited: false,
        can_edit: false,
        actions: {
          can_copy: true,
          can_forward: true,
          can_reply: true,
          can_permalink: true,
          can_view_source: true,
          permalink: `https://matrix.to/#/${encodeURIComponent(roomId)}/${encodeURIComponent(eventId)}`
        },
        reactions: [],
        unable_to_decrypt: {
          session_id: "session-utd",
          reason: "missingRoomKey",
          can_request_keys: true,
          recovery_stage: null,
          recovery_guidance: null
        },
        request_state: null
      };
      void window.__harness.pushCoreEvent({
        kind: "Timeline",
        event: {
          InitialItems: {
            request_id: null,
            key: {
              account_key: userId,
              kind: { Room: { room_id: roomId } }
            },
            generation: 1,
            items: [item]
          }
        }
      });
    },
    { userId: USER_ID, roomId: ROOM_ID, eventId: UTD_EVENT_ID }
  );
}

test("Enter on the request button dispatches request_room_key and announces the toast", async ({
  page
}) => {
  await seedUtdTimeline(page);

  const button = page.getByRole("button", { name: "Request keys and retry" });
  await expect(button).toBeVisible();
  await button.focus();
  await page.keyboard.press("Enter");

  const invocations = await page.evaluate(() =>
    window.__harness.invocationsOf("request_room_key")
  );
  expect(invocations).toHaveLength(1);
  expect(invocations[0].args).toMatchObject({
    roomId: ROOM_ID,
    eventId: UTD_EVENT_ID,
    origin: "user",
    timelineKey: {
      account_key: USER_ID,
      kind: { Room: { room_id: ROOM_ID } }
    }
  });

  const status = page
    .getByRole("main", { name: "Conversation timeline" })
    .getByRole("status");
  await expect(status).toContainText("Decryption key requested");
  await expect(status).toHaveAttribute("aria-live", "polite");
});

test("Space activates the button without dispatching a duplicate command on repeat presses", async ({
  page
}) => {
  await seedUtdTimeline(page);

  const button = page.getByRole("button", { name: "Request keys and retry" });
  await expect(button).toBeVisible();
  await button.focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("Space");

  const invocations = await page.evaluate(() =>
    window.__harness.invocationsOf("request_room_key")
  );
  // One command: the repeat press while pending is suppressed.
  expect(invocations).toHaveLength(1);
});

async function pushHistoryStatus(page: import("@playwright/test").Page, status: import("../src/domain/types").HistoryKeyRecoveryState["status"], japanese = false) {
  const snapshot = await page.evaluate(() => window.__harness.currentSnapshot());
  snapshot.state.domain.room_management.history_keys = { [ROOM_ID]: { generation: 1, status } };
  if (japanese) snapshot.state.domain.locale_profile = { ...snapshot.state.domain.locale_profile, lang: "ja", catalog_locale: "ja" };
  const { pushSnapshot } = await import("./support/stateUpdates");
  await pushSnapshot(page, snapshot);
}

test("historical recovery waits for Rust acknowledgement and permits Retry", async ({ page }) => {
  await seedUtdTimeline(page);
  const button = page.getByRole("button", { name: "Request decryption keys for past messages", exact: true });
  await button.click();
  expect(await page.evaluate(() => window.__harness.invocationsOf("request_history_keys").map(call => call.args))).toEqual([expect.objectContaining({ roomId: ROOM_ID, forceAll: false })]);
  await expect(page.getByText("Requested", { exact: true })).toHaveCount(0);
  await pushHistoryStatus(page, { kind: "sending" });
  await expect(page.getByRole("button", { name: "Requesting…", exact: true })).toBeDisabled();
  await pushHistoryStatus(page, { kind: "requested", recipients: 2, responses: 0, received_keys: 0 });
  await expect(page.getByRole("status").filter({ hasText: /^Requested$/ })).toBeVisible();
  await expect(page.getByText(/Element X does not respond/)).toBeVisible();
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  expect(await page.evaluate(() => window.__harness.invocationsOf("request_history_keys").length)).toBe(2);
  await pushHistoryStatus(page, { kind: "requested", recipients: 1, responses: 0, received_keys: 0 }, true);
  await expect(page.getByRole("status").filter({ hasText: /^要求済みです$/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "再試行", exact: true })).toBeEnabled();
});

test("all-member recovery is hidden inside Room Info troubleshooting", async ({ page }) => {
  await seedUtdTimeline(page);
  const snapshot = await page.evaluate(() => window.__harness.currentSnapshot());
  snapshot.state.domain.rooms = snapshot.state.domain.rooms.map(room => room.room_id === ROOM_ID ? { ...room, is_encrypted: true } : room);
  const { pushSnapshot } = await import("./support/stateUpdates");
  await pushSnapshot(page, snapshot);
  const { t } = await import("../src/i18n/messages");
  await page.getByRole("button", { name: t("room.roomInfo"), exact: true }).click();
  const button = page.getByRole("button", { name: "Request keys from everyone", exact: true });
  await expect(button).not.toBeVisible();
  await page.getByText("Troubleshooting", { exact: true }).click();
  await expect(button).toBeVisible();
  await expect(page.getByText(/Keys already shared cannot be revoked/)).toBeVisible();
  await button.click();
  expect(await page.evaluate(() => window.__harness.invocationsOf("request_history_keys").at(-1)?.args)).toMatchObject({ roomId: ROOM_ID, forceAll: true });
});
