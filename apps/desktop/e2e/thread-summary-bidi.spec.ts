/**
 * Headless spec: the thread summary chip's bidi isolation (#1378).
 *
 * The chip composes `{count} · {sender}: {preview}` with a separately labelled
 * latest-reply timestamp below it (#1556). With the sender and
 * the preview interpolated into one string, an LTR paragraph resolved their
 * trailing neutrals against the template instead of against the value:
 *
 *  - an RTL sender name ending in `42!` had its `!` stranded at the name's right
 *    edge, against the `:` separator, instead of beside its digits;
 *  - an RTL preview dragged the trailing timestamp into its own run, so the time
 *    jumped in front of the preview and the meridiem was left at the far end.
 *
 * The chip now renders the two remote-text values in their own `dir="auto"`
 * elements. These cases measure the rendered glyph positions with a `Range` over
 * the chip's text nodes, so they fail on the interpolated string and pass once
 * the values carry their own direction. All content is synthetic.
 */

import { expect, test, type Page } from "@playwright/test";
import { gotoReadyShell, seedTimelineItems } from "./support/basicOperations";

/** RTL sender label ending in digits and punctuation, as in the report. */
const RTL_SENDER = "דוגמה 42!";
const RTL_PREVIEW = "תשובה 7!";
const LTR_SENDER = "Member One";
const LTR_PREVIEW = "Synthetic reply";

interface Rect {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

interface ChipGeometry {
  text: string;
  ariaLabel: string;
  /** Edges of each requested needle's first occurrence, plus the separate latest-reply time. */
  rects: Record<string, Rect>;
  time: Rect;
}

/**
 * Measures character positions inside a chip by spanning the chip's own text
 * nodes, so the result does not depend on how the chip is split into elements.
 */
async function chipGeometry(
  page: Page,
  eventId: string,
  needles: readonly string[]
): Promise<ChipGeometry> {
  const measured = await page.evaluate(
    ({ rowId, wanted }) => {
      const row = document.querySelector<HTMLElement>(`[data-item-id="${CSS.escape(rowId)}"]`);
      const chip = row?.querySelector<HTMLElement>(".thread-summary-chip");
      if (!chip) {
        return null;
      }
      const walker = document.createTreeWalker(chip, NodeFilter.SHOW_TEXT);
      const nodes: Array<{ node: Text; start: number }> = [];
      let text = "";
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        nodes.push({ node: node as Text, start: text.length });
        text += (node as Text).data;
      }
      /** The union rectangle of `length` chip characters starting at `start`. */
      const measure = (start: number, length: number) => {
        const locate = (index: number) => {
          for (let i = nodes.length - 1; i >= 0; i -= 1) {
            const entry = nodes[i]!;
            if (index >= entry.start) {
              return { node: entry.node, offset: index - entry.start };
            }
          }
          return null;
        };
        const from = locate(start);
        const to = locate(start + length - 1);
        if (!from || !to) {
          return null;
        }
        const range = document.createRange();
        range.setStart(from.node, from.offset);
        range.setEnd(to.node, to.offset + 1);
        const rect = range.getBoundingClientRect();
        return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
      };
      const rects: Record<string, Rect> = {};
      for (const needle of wanted) {
        const start = text.indexOf(needle);
        const rect = start < 0 ? null : measure(start, needle.length);
        if (rect) {
          rects[needle] = rect;
        }
      }
      const time = row?.querySelector<HTMLElement>(".thread-latest-reply time");
      const timeRect = time?.getBoundingClientRect();
      return {
        text,
        ariaLabel: chip.getAttribute("aria-label") ?? "",
        rects,
        time: timeRect ? { left: timeRect.left, right: timeRect.right, top: timeRect.top, bottom: timeRect.bottom } : null
      };
    },
    { rowId: eventId, wanted: [...needles] }
  );
  if (!measured) {
    throw new Error(`no thread summary chip in row ${eventId}`);
  }
  if (!measured.time) {
    throw new Error(`thread summary carries no latest-reply timestamp`);
  }
  for (const needle of needles) {
    if (!measured.rects[needle]) {
      throw new Error(`chip text ${JSON.stringify(measured.text)} does not contain ${needle}`);
    }
  }
  return measured as ChipGeometry;
}

function edge(chip: ChipGeometry, needle: string, which: keyof Rect): number {
  const rect = chip.rects[needle];
  if (!rect) {
    throw new Error(`no measurement for ${needle}`);
  }
  return rect[which];
}

/** A thread root whose chip shows the given latest sender label and preview. */
function threadRootRow(
  eventId: string,
  senderLabel: string,
  preview: string
): Record<string, unknown> {
  return {
    id: { Event: { event_id: eventId } },
    sender: "@harness-bob:example.invalid",
    body: "Synthetic thread root",
    timestamp_ms: 1_800_000_000_000,
    in_reply_to_event_id: null,
    thread_root: null,
    thread_summary: {
      reply_count: 1,
      unread_count: 0,
      latest_event_id: `$latest-${eventId}`,
      latest_sender: "@harness-ada:example.invalid",
      latest_sender_label: senderLabel,
      latest_body_preview: preview,
      latest_timestamp_ms: 1_800_000_060_000
    },
    reactions: [],
    can_react: false,
    is_redacted: false,
    is_hidden: false,
    can_redact: false,
    is_edited: false,
    can_edit: false
  };
}

test.beforeEach(async ({ page }) => {
  await gotoReadyShell(page);
});

test("an RTL sender name keeps its trailing punctuation beside its digits", async ({ page }) => {
  const eventId = "$bidi-rtl-sender:example.invalid";
  await seedTimelineItems(page, [threadRootRow(eventId, RTL_SENDER, LTR_PREVIEW)]);

  const chip = await chipGeometry(page, eventId, ["!", "42", `: ${LTR_PREVIEW}`]);
  expect(chip.text).toContain(`${RTL_SENDER}: ${LTR_PREVIEW}`);

  // The name is laid out right to left, so its `!` is the leftmost character of
  // the name and sits left of the digits it belongs with.
  expect(edge(chip, "!", "left")).toBeLessThan(edge(chip, "42", "left"));
  // The template's own `:` still follows the name block.
  expect(edge(chip, "!", "left")).toBeLessThan(edge(chip, `: ${LTR_PREVIEW}`, "left"));
});

test("an RTL preview does not pull the timestamp into its own run", async ({ page }) => {
  const eventId = "$bidi-rtl-preview:example.invalid";
  await seedTimelineItems(page, [threadRootRow(eventId, LTR_SENDER, RTL_PREVIEW)]);

  const chip = await chipGeometry(page, eventId, [RTL_PREVIEW]);
  // The timestamp has its own line and cannot join the remote preview's bidi run.
  expect(edge(chip, RTL_PREVIEW, "bottom")).toBeLessThanOrEqual(chip.time.top);
});

test("an RTL sender and an RTL preview stay in the template's order", async ({ page }) => {
  const eventId = "$bidi-both-rtl:example.invalid";
  await seedTimelineItems(page, [threadRootRow(eventId, RTL_SENDER, RTL_PREVIEW)]);

  const chip = await chipGeometry(page, eventId, [RTL_SENDER, RTL_PREVIEW]);
  // Name block, then the preview block: with the sender isolated, the preview is
  // no longer shown before the name.
  expect(edge(chip, RTL_SENDER, "right")).toBeLessThanOrEqual(edge(chip, RTL_PREVIEW, "left") + 1);
  // The accessible name cannot carry elements, so the same two values are
  // isolated with FSI (U+2068) and PDI (U+2069).
  expect(chip.ariaLabel).toContain(`\u2068${RTL_SENDER}\u2069`);
  expect(chip.ariaLabel).toContain(`\u2068${RTL_PREVIEW}\u2069`);
});

test("an LTR sender and preview keep the plain order", async ({ page }) => {
  const eventId = "$bidi-ltr-control:example.invalid";
  await seedTimelineItems(page, [threadRootRow(eventId, LTR_SENDER, LTR_PREVIEW)]);

  const chip = await chipGeometry(page, eventId, [LTR_SENDER, LTR_PREVIEW]);
  expect(edge(chip, LTR_SENDER, "left")).toBeLessThan(edge(chip, LTR_PREVIEW, "left"));
  expect(edge(chip, LTR_PREVIEW, "bottom")).toBeLessThanOrEqual(chip.time.top);
});
