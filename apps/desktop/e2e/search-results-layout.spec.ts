/**
 * Headless spec: search-result layout in the narrow right panel (#318).
 *
 * The right Search panel is much narrower than the main pane. The result row
 * used to be a two-column grid whose metadata column was `auto` + `nowrap`, so
 * the metadata reserved its full unwrapped width and left the snippet a sliver
 * — snippets wrapped roughly one word per line and the panel scrolled
 * horizontally.
 *
 * These assertions measure rendered geometry rather than reading CSS text, so
 * they fail on the symptom the user reported rather than on a rule name.
 */

import { expect, test, type Page } from "@playwright/test";
import { t } from "../src/i18n/messages";

const HARNESS_ROOM_ID = "!harness-room:example.invalid";
/** Long enough that an unwrapped metadata column would starve the snippet. */
const LONG_ROOM_NAME = "Harness Engineering Planning And Retrospective";
const LONG_SNIPPET =
  "True - it will probably require a refactoring pass over the projection layer " +
  "before the crawler can reuse it without duplicating the candidate store.";

async function gotoReadyShell(page: Page): Promise<void> {
  await page.goto("/appHarness.html");
  await expect(page.getByRole("main", { name: t("timeline.conversation") })).toBeVisible();
}

/** Seed a Rust-shaped search result and open the right Search panel. */
async function openSearchPanelWithResult(page: Page): Promise<void> {
  await page.evaluate(
    ({ roomId, roomName, snippet }) => {
      window.__harness.setCommandResponse("submit_search", ({ query }: { query?: string }) => {
        const next = window.__harness.currentSnapshot();
        return {
          ...next,
          state: {
            ...next.state,
            domain: {
              ...next.state.domain,
              rooms: next.state.domain.rooms.map((room) =>
                room.room_id === roomId
                  ? {
                      ...room,
                      display_name: roomName,
                      display_label: roomName,
                      original_display_label: roomName
                    }
                  : room
              ),
              search: {
                kind: "results",
                request_id: 91,
                query: String(query ?? "refactoring"),
                scope: "allRooms",
                results: [
                  {
                    room_id: roomId,
                    event_id: "$search-layout:example.invalid",
                    sender: "@harness-ada:example.invalid",
                    timestamp_ms: 1_800_000_004_000,
                    score_millis: 990,
                    snippet,
                    match_field: "messageBody",
                    highlights: [{ start_utf16: 46, end_utf16: 57 }],
                    match_kind: "exact"
                  }
                ]
              }
            }
          }
        };
      });
    },
    { roomId: HARNESS_ROOM_ID, roomName: LONG_ROOM_NAME, snippet: LONG_SNIPPET }
  );

  await page.locator(".top-search input").fill("refactoring");
  await expect(page.locator(".thread-pane.search-panel .result-button").first()).toBeVisible();
}

test("narrow search panel keeps the snippet readable and the row inside its width", async ({
  page
}) => {
  // A narrow window is the reported condition; the right panel is narrower still.
  await page.setViewportSize({ width: 1024, height: 800 });
  await gotoReadyShell(page);
  await openSearchPanelWithResult(page);

  const geometry = await page.evaluate(() => {
    const row = document.querySelector<HTMLElement>(
      ".thread-pane.search-panel .result-button"
    );
    const list = document.querySelector<HTMLElement>(
      ".thread-pane.search-panel .result-list"
    );
    if (!row || !list) {
      return null;
    }
    const meta = row.querySelector<HTMLElement>(".result-meta");
    // The snippet is the row's first span; the metadata is the labelled one.
    const snippet = Array.from(row.querySelectorAll<HTMLElement>(":scope > span")).find(
      (node) => node !== meta
    );
    if (!meta || !snippet) {
      return null;
    }
    const rowStyle = window.getComputedStyle(row);
    const contentWidth =
      row.getBoundingClientRect().width -
      parseFloat(rowStyle.paddingInlineStart) -
      parseFloat(rowStyle.paddingInlineEnd);
    return {
      contentWidth,
      snippetWidth: snippet.getBoundingClientRect().width,
      snippetHeight: snippet.getBoundingClientRect().height,
      snippetBottom: snippet.getBoundingClientRect().bottom,
      metaTop: meta.getBoundingClientRect().top,
      listScrollWidth: list.scrollWidth,
      listClientWidth: list.clientWidth,
      docScrollWidth: document.documentElement.scrollWidth,
      docClientWidth: document.documentElement.clientWidth
    };
  });

  expect(geometry).not.toBeNull();
  const g = geometry!;

  // The snippet must get essentially the whole row, not what is left after an
  // unwrapped metadata column.
  expect(g.snippetWidth).toBeGreaterThan(g.contentWidth * 0.9);

  // Metadata belongs on its own line, below the snippet — never beside or over it.
  expect(g.metaTop).toBeGreaterThanOrEqual(g.snippetBottom - 1);

  // No horizontal scrollbar in the panel or the document.
  expect(g.listScrollWidth).toBeLessThanOrEqual(g.listClientWidth + 1);
  expect(g.docScrollWidth).toBeLessThanOrEqual(g.docClientWidth + 1);

  // A sliver-width snippet wraps word-per-line and grows very tall. With the
  // full row width this text needs only a few lines.
  expect(g.snippetHeight).toBeLessThan(140);
});
