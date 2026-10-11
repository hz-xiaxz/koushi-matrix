/**
 * Headless spec: the thread pane renders Element-style user mentions as pills
 * (#1328, following the mention-pill contract of #874/#1123).
 *
 * Element writes a mention as `m.mentions.user_ids` plus a `matrix.to` user
 * anchor in the formatted body. That anchor is neither percent-encoded nor
 * prefixed with `@`, so the thread pane must not depend on either: the mention
 * facts come from the event, and the anchor is only the visible label.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";
import { threadTimelineKey } from "../src/domain/coreEvents";
import { gotoReadyShell } from "./support/basicOperations";

const HARNESS_ACCOUNT_KEY = "@harness-user:example.invalid";
const HARNESS_ROOM_ID = "!harness-room:example.invalid";
const ROOT_EVENT_ID = "$seed-event:example.invalid";
const MENTIONED_USER_ID = "@harness-ada:example.invalid";
const MENTION_LABEL = "Harness Ada";
/** Element's own shape: a bare `matrix.to` href, label without a leading `@`. */
const ELEMENT_MENTION_HREF = `https://matrix.to/#/${MENTIONED_USER_ID}`;

/** Opens the thread pane through the UI, then delivers the open snapshot. */
async function openThreadPane(page: Page): Promise<void> {
  await page.getByRole("button", { name: /2 replies/ }).click();
  await page.evaluate(
    ({ roomId, rootEventId }) => {
      const snapshot = window.__harness.currentSnapshot();
      window.__harness.setCommandResponse("open_thread", {
        ...snapshot,
        state: {
          ...snapshot.state,
          ui: {
            ...snapshot.state.ui,
            thread: {
              kind: "opening",
              room_id: roomId,
              root_event_id: rootEventId,
              intent: "existingThread"
            }
          }
        }
      });
      window.__harness.setSnapshot({
        ...snapshot,
        state: {
          ...snapshot.state,
          ui: {
            ...snapshot.state.ui,
            thread: {
              kind: "open",
              room_id: roomId,
              root_event_id: rootEventId,
              intent: "existingThread",
              is_subscribed: true,
              staged_uploads: [],
              composer: { ...snapshot.state.ui.timeline.composer }
            }
          }
        }
      });
      window.__harness.pushStateUpdate();
    },
    { roomId: HARNESS_ROOM_ID, rootEventId: ROOT_EVENT_ID }
  );
  const panel = page.getByRole("complementary", { name: t("panel.context") });
  await expect(panel.getByText(t("panel.thread"), { exact: true })).toBeVisible();
}

function threadItem(
  eventId: string,
  body: string,
  overrides: Record<string, unknown> = {}
): Record<string, unknown> {
  return {
    id: { Event: { event_id: eventId } },
    sender: "@harness-bob:example.invalid",
    sender_label: "Harness Bob",
    body,
    timestamp_ms: 1_800_000_100_000,
    in_reply_to_event_id: null,
    thread_root: ROOT_EVENT_ID,
    thread_summary: null,
    reactions: [],
    can_react: true,
    is_redacted: false,
    is_hidden: false,
    can_redact: true,
    is_edited: false,
    can_edit: true,
    ...overrides
  };
}

const MENTIONED_BODY = "Harness Ada please take a look";

async function seedThreadItems(page: Page, items: unknown[]): Promise<void> {
  const key = threadTimelineKey(HARNESS_ACCOUNT_KEY, HARNESS_ROOM_ID, ROOT_EVENT_ID);
  await expect
    .poll(
      async () =>
        page.evaluate(
          async ({ timelineKey, nextItems }) => {
            await window.__harness.pushCoreEvent({
              kind: "Timeline",
              event: {
                InitialItems: { request_id: null, key: timelineKey, generation: 2, items: nextItems }
              }
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
            } as any);
            await new Promise((resolve) => setTimeout(resolve, 25));
            return nextItems.every((item) =>
              document.querySelector(
                `[data-event-id="${CSS.escape(
                  (item as { id: { Event: { event_id: string } } }).id.Event.event_id
                )}"]`
              )
            );
          },
          { timelineKey: key, nextItems: items }
        ),
      { timeout: 10_000, intervals: [25, 50, 100, 250] }
    )
    .toBe(true);
}

test("a thread reply that mentions a user renders the mention as a pill", async ({ page }) => {
  await gotoReadyShell(page);
  await openThreadPane(page);

  await seedThreadItems(page, [
    threadItem(ROOT_EVENT_ID, "thread root", { thread_root: null }),
    // Element-authored mention: `m.mentions` plus its unencoded anchor.
    threadItem("$thread-mentioned:example.invalid", MENTIONED_BODY, {
      formatted: {
        html: `<a href="${ELEMENT_MENTION_HREF}">${MENTION_LABEL}</a> please take a look`,
        plain_text: MENTIONED_BODY,
        code_blocks: []
      },
      mentioned_user_ids: [MENTIONED_USER_ID]
    }),
    // Same visible text, no `m.mentions`: it notifies nobody and is not a pill.
    threadItem("$thread-unmentioned:example.invalid", MENTIONED_BODY)
  ]);

  const panel = page.getByRole("complementary", { name: t("panel.context") });
  const mentionedRow = panel.locator('[data-event-id="$thread-mentioned:example.invalid"]');
  const pill = mentionedRow.locator("a.message-mention-pill");
  await expect(pill).toHaveCount(1);
  await expect(pill).toHaveAttribute("data-mention-user-id", MENTIONED_USER_ID);
  await expect(pill).toHaveText(MENTION_LABEL);

  const unmentionedRow = panel.locator('[data-event-id="$thread-unmentioned:example.invalid"]');
  await expect(unmentionedRow).toBeVisible();
  await expect(unmentionedRow.locator(".message-mention-pill")).toHaveCount(0);
  await expect(unmentionedRow).toContainText(MENTIONED_BODY);
});
