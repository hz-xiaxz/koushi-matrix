/**
 * Headless spec: an arrow press crosses a mention pill, not an invisible caret
 * anchor (#956).
 *
 * The #875 caret anchors are zero-width text the composer collapses to the
 * document offset beside them, so the engine treated each one as its own caret
 * stop: with two adjacent pills, three of five Right presses moved the caret
 * nowhere the user could see. Only the real engine has native caret motion, so
 * this is the tier that can observe the regression.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

const ROOM_ID = "!harness-room:example.invalid";

async function seedMentionCandidates(page: Page): Promise<void> {
  await page.evaluate((roomId) => {
    const candidates = [
      {
        user_id: "@mention-0:example.invalid",
        display_label: "Mention Person 0",
        original_display_label: "Mention Person 0",
        avatar: null,
        membership: "joined"
      },
      {
        user_id: "@mention-1:example.invalid",
        display_label: "Mention Person 1",
        original_display_label: "Mention Person 1",
        avatar: null,
        membership: "joined"
      }
    ];
    const withCandidates = (snapshot: typeof window.__harness.currentSnapshot) => ({
      ...snapshot,
      state: {
        ...snapshot.state,
        domain: {
          ...snapshot.state.domain,
          mention_candidates: {
            targets: [
              {
                room_id: roomId,
                generation: 1,
                request_id: 956,
                query: "",
                surface: "main",
                completeness: "complete",
                candidates,
                room_mention_allowed: "denied",
                failure_kind: null
              }
            ]
          }
        }
      }
    });
    const seeded = withCandidates(window.__harness.currentSnapshot());
    window.__harness.setSnapshot(seeded);
    window.__harness.setCommandResponse("query_mention_candidates", () => seeded);
    window.__harness.pushStateUpdate();
  }, ROOM_ID);
}

/** Insert one pill and delete the trailing space the composer adds after it. */
async function insertAdjacentMention(page: Page, index: number): Promise<void> {
  await page.keyboard.type("@");
  const option = page.locator(".composer-autocomplete [role='option']").nth(index);
  await expect(option).toBeVisible();
  await option.click();
  await page.keyboard.press("Backspace");
}

/** The caret's x position and both pills' border boxes. */
async function caretAndPills(page: Page) {
  return page.evaluate(() => {
    const control = document.querySelector<HTMLElement>(".composer-inline-editor");
    const pills = Array.from(
      control?.querySelectorAll<HTMLElement>("[data-composer-mention]") ?? []
    );
    const selection = document.getSelection();
    if (!control || pills.length !== 2 || !selection || selection.rangeCount === 0) {
      throw new Error("composer mention selection unavailable");
    }
    return {
      caretX: selection.getRangeAt(0).getBoundingClientRect().x,
      firstPillRight: pills[0].getBoundingClientRect().right,
      secondPillRight: pills[1].getBoundingClientRect().right
    };
  });
}

test("each arrow press crosses a mention pill boundary", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
  await seedMentionCandidates(page);

  const composer = page.getByRole("textbox", { name: t("composer.messageComposer") });
  await composer.click();
  await insertAdjacentMention(page, 0);
  await insertAdjacentMention(page, 1);

  // Home puts the caret before the first pill; from there every press must move
  // the caret past exactly one pill.
  await page.keyboard.press("Home");
  const start = await caretAndPills(page);
  expect(start.caretX).toBeLessThan(start.firstPillRight);

  await page.keyboard.press("ArrowRight");
  const afterFirst = await caretAndPills(page);
  expect(afterFirst.caretX).toBeGreaterThanOrEqual(afterFirst.firstPillRight);
  expect(afterFirst.caretX).toBeLessThan(afterFirst.secondPillRight);

  await page.keyboard.press("ArrowRight");
  const afterSecond = await caretAndPills(page);
  expect(afterSecond.caretX).toBeGreaterThanOrEqual(afterSecond.secondPillRight);

  // Backward travel costs one press per pill too.
  await page.keyboard.press("ArrowLeft");
  const backOne = await caretAndPills(page);
  expect(backOne.caretX).toBeGreaterThanOrEqual(backOne.firstPillRight);
  expect(backOne.caretX).toBeLessThan(backOne.secondPillRight);

  await page.keyboard.press("ArrowLeft");
  const backTwo = await caretAndPills(page);
  expect(backTwo.caretX).toBeLessThan(backTwo.firstPillRight);
});
