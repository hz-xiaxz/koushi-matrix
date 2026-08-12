/**
 * Headless spec: the mention autocomplete popup keeps the keyboard-selected
 * option visible while Arrow Up/Down navigate, without scrolling the outer
 * page (#480).
 *
 * Before the fix, Arrow Down could move the active option below the popup's
 * visible area (`.composer-autocomplete` is 240px tall with `overflow: auto`)
 * with no scroll to follow it, so keyboard users lost the selection.
 *
 * These tests seed 30 mention candidates through the real snapshot and drive
 * the real composer with Arrow keys, then measure popup `scrollTop` (must
 * change) while neither the page nor a scrollable timeline ancestor moves.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

const ROOM_ID = "!harness-room:example.invalid";
const CANDIDATE_COUNT = 30;

/** Seed 30 mention candidates for the active room + main composer surface. */
async function seedMentionCandidates(page: Page): Promise<void> {
  await page.evaluate(
    ({ roomId, count }) => {
      const candidates = Array.from({ length: count }, (_, index) => ({
        user_id: `@mention-${index}:example.invalid`,
        display_label: `Mention Person ${index}`,
        original_display_label: `Mention Person ${index}`,
        avatar: null,
        membership: "joined"
      }));
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
                  request_id: 480,
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
      const snapshot = window.__harness.currentSnapshot();
      const seeded = withCandidates(snapshot);
      window.__harness.setSnapshot(seeded);
      // The harness's own query_mention_candidates response would replace the
      // seeded list with a single candidate; override it to keep all 30.
      window.__harness.setCommandResponse("query_mention_candidates", () => seeded);
      window.__harness.pushStateChanged();
    },
    { roomId: ROOM_ID, count: CANDIDATE_COUNT }
  );
}

async function popupScrollTop(page: Page): Promise<number> {
  return page.locator(".composer-autocomplete").evaluate((element) => element.scrollTop);
}

/**
 * Scroll positions that must never move while the popup scrolls: the page
 * itself and the timeline message scroller (the popup's real scrollable
 * ancestors — `window.scrollY` alone is vacuous because the body cannot
 * scroll).
 */
async function outerScrollPositions(page: Page): Promise<{ page: number; timeline: number }> {
  return page.evaluate(() => {
    const timeline = document.querySelector<HTMLElement>(".timeline-scroll");
    return {
      page: window.scrollY,
      timeline: timeline?.scrollTop ?? 0
    };
  });
}

/** Is the active option fully inside the popup's visible box? */
async function activeOptionVisible(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const popup = document.querySelector<HTMLElement>(".composer-autocomplete");
    const active = popup?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (!popup || !active) {
      return false;
    }
    const popupRect = popup.getBoundingClientRect();
    const optionRect = active.getBoundingClientRect();
    return (
      optionRect.top >= popupRect.top - 1 && optionRect.bottom <= popupRect.bottom + 1
    );
  });
}

async function activeOptionIndex(page: Page): Promise<number> {
  return page.evaluate(() => {
    const active = document.querySelector<HTMLElement>(".composer-autocomplete [aria-selected=\"true\"]");
    if (!active) {
      return -1;
    }
    return Number(active.id.split("-option-").at(-1));
  });
}

test("mention popup scrolls to keep the active option visible; the page never scrolls", async ({
  page
}) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
  await seedMentionCandidates(page);

  const composer = page.getByRole("textbox", { name: t("composer.messageComposer") });
  await composer.fill("@");
  const listbox = page.getByRole("listbox", { name: t("composer.mentionSuggestions") });
  await expect(listbox).toBeVisible();
  await expect(listbox.getByRole("option")).toHaveCount(CANDIDATE_COUNT);

  expect(await popupScrollTop(page)).toBe(0);
  expect(await outerScrollPositions(page)).toEqual({ page: 0, timeline: 0 });

  // Arrow Down past the popup's lower edge: the popup must scroll, the page
  // must not.
  for (let step = 0; step < 12; step += 1) {
    await page.keyboard.press("ArrowDown");
  }
  await expect.poll(() => popupScrollTop(page)).toBeGreaterThan(0);
  expect(await activeOptionVisible(page)).toBe(true);
  expect(await outerScrollPositions(page)).toEqual({ page: 0, timeline: 0 });

  // Wraparound from the last option back to the first: the active option is
  // fully visible again, back in the top region of the popup (the first option
  // sits below its section heading, so the popup stops there rather than at 0).
  for (let step = 0; step < CANDIDATE_COUNT; step += 1) {
    await page.keyboard.press("ArrowDown");
    if ((await activeOptionIndex(page)) === 0) {
      break;
    }
  }
  expect(await activeOptionVisible(page)).toBe(true);
  const topRegion = await page.locator(".composer-autocomplete").evaluate((element) => {
    const first = element.querySelector<HTMLElement>(".composer-autocomplete-option");
    return first ? element.scrollTop < first.offsetHeight : element.scrollTop === 0;
  });
  expect(topRegion, "wraparound must return the list to its top region").toBe(true);
  expect(await outerScrollPositions(page)).toEqual({ page: 0, timeline: 0 });

  // Wraparound from the first option up to the last scrolls to the bottom.
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => popupScrollTop(page)).toBeGreaterThan(0);
  expect(await activeOptionVisible(page)).toBe(true);
  expect(await outerScrollPositions(page)).toEqual({ page: 0, timeline: 0 });
});
