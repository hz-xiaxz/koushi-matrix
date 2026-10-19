/**
 * Website screenshot lane: renders the real React <App /> in the browser harness
 * with synthetic, localized state, once per site locale and scene. Output goes to
 * site/screenshots/<locale>/<scene>.png, which site/build.mjs requires.
 *
 * The search scenes render site/generated/search-scenes.json, which a koushi-core
 * test produces from the production index and query path; nothing here invents a
 * search hit.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { catalogs, type MessageId } from "../../src/i18n/messages";
import type { TimelineItem } from "../../src/domain/coreEvents";
import type { DesktopSnapshot, NotificationSinkSettings, RoomSummary, SearchResult } from "../../src/domain/types";
import { createReadmeFixture, type ReadmeFixture } from "../readmeFixture";
import { SITE_LOCALES, SITE_TEXT, type SiteLocale } from "./siteText";

const OUT_DIR = fileURLToPath(new URL("../../../../site/screenshots/", import.meta.url));
const SEARCH_SCENES = JSON.parse(
  readFileSync(fileURLToPath(new URL("../../../../site/generated/search-scenes.json", import.meta.url)), "utf8")
) as Record<string, { query: string; results: RustSearchResult[] }>;

/** koushi_state::SearchResult as serde writes it (PascalCase enum variants). */
interface RustSearchResult {
  room_id: string;
  event_id: string;
  context_label?: string | null;
  sender: string;
  timestamp_ms: number;
  score_millis: number;
  snippet: string;
  match_field: "MessageBody" | "AttachmentFileName";
  highlights: { start_utf16: number; end_utf16: number }[];
  match_kind: "Exact";
}

/** Mirror of FrontendSearchResult::from in apps/desktop/src-tauri/src/dto.rs. */
function toFrontendSearchResult(result: RustSearchResult): SearchResult {
  const { context_label, ...rest } = result;
  return {
    ...rest,
    ...(context_label == null ? {} : { context_label }),
    match_field: result.match_field === "MessageBody" ? "messageBody" : "attachmentFileName",
    match_kind: "exact"
  } as SearchResult;
}

async function settleLayout(page: Page): Promise<void> {
  await page.evaluate(
    () => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
  );
}

/**
 * Fails when CJK text would render as tofu. Glyph widths cannot tell (both are one
 * em), so each sample character's pixels are compared with the missing-glyph box
 * drawn for an unassigned code point.
 */
async function expectCjkGlyphs(page: Page): Promise<void> {
  const tofuLike = await page.evaluate(async () => {
    await document.fonts.ready;
    const family = getComputedStyle(document.body).fontFamily;
    const pixels = (character: string) => {
      const canvas = document.createElement("canvas");
      canvas.width = 48;
      canvas.height = 48;
      const context = canvas.getContext("2d")!;
      context.font = `32px ${family}`;
      context.textBaseline = "top";
      context.fillText(character, 4, 4);
      return Array.from(context.getImageData(0, 0, 48, 48).data).join(",");
    };
    const tofu = pixels("\u{10FFFF}");
    return Array.from("漢か简體한").filter((character) => pixels(character) === tofu);
  });
  expect(tofuLike, "CJK characters rendered as the missing-glyph box; install CJK fonts").toEqual([]);
}

const EXPECTED_CJK_FAMILY: Partial<Record<SiteLocale, string>> = {
  ja: "Noto Sans CJK JP",
  ko: "Noto Sans CJK KR",
  "zh-Hans": "Noto Sans CJK SC",
  "zh-Hant": "Noto Sans CJK TC"
};

/**
 * Chromium picks a CJK fallback font per locale; the Playwright image's bundled
 * Chinese font would otherwise render Japanese and Korean too. Asks DevTools which
 * fonts actually drew the first message (site/prepare-screenshot-fonts.sh).
 */
async function expectLocaleCjkFont(page: Page, locale: SiteLocale, target: Locator): Promise<void> {
  const expected = EXPECTED_CJK_FAMILY[locale];
  if (!expected) {
    return;
  }
  await target.evaluate((element) => element.setAttribute("data-site-font-probe", ""));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 });
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: "[data-site-font-probe]" });
  const { fonts } = await cdp.send("CSS.getPlatformFontsForNode", { nodeId });
  await target.evaluate((element) => element.removeAttribute("data-site-font-probe"));
  await cdp.detach();
  const families = fonts.map((font) => font.familyName);
  const message = `${locale} text must render in ${expected}, got ${families.join(", ")}; run site/prepare-screenshot-fonts.sh`;
  expect(families, message).toContain(expected);
  // Other CJK families mean some characters fell back to the wrong language's font.
  expect(families.filter((family) => /CJK|WenQuanYi|IPA|Unifont|Droid Sans Fallback/.test(family) && family !== expected), message).toEqual([]);
}

function label(locale: SiteLocale, id: MessageId): string {
  return catalogs[locale][id];
}

function searchRoom(roomId: string, name: string): RoomSummary {
  return {
    room_id: roomId,
    display_name: name,
    display_label: name,
    original_display_label: name,
    avatar: null,
    is_dm: false,
    dm_user_ids: [],
    tags: { favourite: null, low_priority: null },
    unread_count: 0,
    notification_count: 0,
    highlight_count: 0,
    latest_event: null,
    parent_space_ids: [],
    dm_space_ids: [],
    is_encrypted: true,
    joined_members: 6
  };
}

const SYNTHETIC_SINK_BASE: Omit<NotificationSinkSettings, "id" | "config" | "destination" | "from_address"> = {
  sender_name: "Koushi",
  secret_binding: null,
  targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
  content: { kind: "namesOnly" },
  content_consent: null,
  direct_partner_label: "localAlias",
  timing: { dwell_secs: 300, min_interval_secs: 900, only_when_unfocused: true }
};

const SYNTHETIC_SINKS: NotificationSinkSettings[] = [
  {
    ...SYNTHETIC_SINK_BASE,
    id: "sink-site-resend",
    destination: "me@example.invalid",
    from_address: "koushi@example.invalid",
    config: { kind: "resend", endpoint: "https://api.example.invalid/emails" }
  },
  {
    ...SYNTHETIC_SINK_BASE,
    id: "sink-site-smtp",
    destination: "me@work.example.invalid",
    from_address: "me@work.example.invalid",
    config: {
      kind: "smtp",
      preset: null,
      host: "relay.example.invalid",
      port: 587,
      tls: "startTls",
      username: "me"
    }
  },
  {
    ...SYNTHETIC_SINK_BASE,
    id: "sink-site-serverchan",
    destination: "",
    from_address: "",
    config: { kind: "serverChan", endpoint: "https://sctapi.example.invalid" }
  }
];

function richItem(locale: SiteLocale, currentUserId: string): TimelineItem {
  const rich = SITE_TEXT[locale].rich;
  const code = 'let hits = index.search("Koushi");';
  const html =
    `<p><strong>${rich.intro}</strong></p><ul><li>${rich.item1}</li><li><code>cargo test</code> — ${rich.item2}</li></ul>` +
    `<pre><code class="language-rust">${code.replaceAll('"', "&quot;")}</code></pre>` +
    `<p>${rich.outro} <span data-mx-maths="s = \\sum_i w_i \\, \\mathrm{tf}_i">s</span>.</p>`;
  return {
    id: { Event: { event_id: "$site-rich:example.invalid" } },
    sender: currentUserId,
    sender_label: SITE_TEXT[locale].fixture.people.you,
    body: `${rich.intro} ${rich.item1} ${rich.item2} ${code}`,
    formatted: { html, plain_text: `${rich.intro} ${rich.item1} ${rich.item2} ${code}`, code_blocks: [{ language: "rust", body: code }] },
    timestamp_ms: Date.UTC(2026, 2, 10, 9, 45),
    in_reply_to_event_id: null,
    reply_quote: null,
    thread_root: null,
    thread_summary: null,
    reactions: [],
    can_react: true,
    is_redacted: false,
    is_hidden: false,
    can_redact: true,
    is_edited: false,
    can_edit: true
  };
}

interface SceneOptions {
  mutateSnapshot?: (snapshot: DesktopSnapshot) => void;
  mutateItems?: (items: TimelineItem[], currentUserId: string) => TimelineItem[];
}

/** Opens the harness in `locale` and pushes the localized README composition. */
async function openWorkspace(page: Page, locale: SiteLocale, options: SceneOptions = {}): Promise<ReadmeFixture> {
  await page.goto("/appHarness.html");
  await page.addStyleTag({ content: ".sync-status-server { display: none !important; }" });
  await expect(page.getByRole("main")).toBeVisible();
  await page.evaluate(
    (languageTag) =>
      window.__harness.invoke("update_settings", {
        patch: { scope: "app", locale: { language_tag: languageTag, text_direction: "auto" } }
      }),
    locale
  );
  const source = await page.evaluate(() => window.__harness.currentSnapshot());
  const fixture = createReadmeFixture(source, SITE_TEXT[locale].fixture);
  const session = source.state.domain.session;
  if (session.kind !== "ready") {
    throw new Error("site screenshots require the ready harness session");
  }
  options.mutateSnapshot?.(fixture.snapshot);
  if (options.mutateItems) {
    const event = fixture.initialItems as { kind: "Timeline"; event: { InitialItems: { items: TimelineItem[] } } };
    event.event.InitialItems.items = options.mutateItems(event.event.InitialItems.items, session.user_id);
  }
  await page.evaluate(async ({ accountTabs, stateUpdate, initialItems }) => {
    await window.__harness.pushAccountTabs(accountTabs);
    window.__harness.pushStateUpdate(stateUpdate);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await window.__harness.pushCoreEvent(initialItems);
  }, fixture);
  await expect(page.locator("html")).toHaveAttribute("lang", locale);
  await expect(
    page.locator("article.message[data-event-id] .message-body").filter({ hasText: SITE_TEXT[locale].fixture.messages[0] })
  ).toHaveCount(1);
  await expectCjkGlyphs(page);
  await expectLocaleCjkFont(
    page,
    locale,
    page.locator("article.message[data-event-id] .message-body").filter({ hasText: SITE_TEXT[locale].fixture.messages[0] })
  );
  return fixture;
}

async function capture(page: Page, locale: SiteLocale, scene: string, target?: Locator): Promise<void> {
  await settleLayout(page);
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await settleLayout(page);
  const path = `${OUT_DIR}${locale}/${scene}.png`;
  if (target) {
    await target.screenshot({ path, animations: "disabled", caret: "hide" });
  } else {
    await page.screenshot({ path, animations: "disabled", caret: "hide" });
  }
}

for (const locale of SITE_LOCALES) {
  test.describe(`site screenshots (${locale})`, () => {
    test("main", async ({ page }) => {
      await openWorkspace(page, locale);
      await expect(page.locator(".account-tab-strip .account-tab")).toHaveCount(3);
      await capture(page, locale, "main");
    });

    test("rich", async ({ page }) => {
      await openWorkspace(page, locale, {
        mutateItems: (items, userId) => [...items.slice(0, -2), richItem(locale, userId)]
      });
      // The production entry (src/main.tsx) imports KaTeX's stylesheet; the harness entry does not.
      await page.addStyleTag({ url: "/node_modules/katex/dist/katex.min.css" });
      const rich = page.locator('[data-event-id="$site-rich:example.invalid"]');
      await expect(rich.locator(".katex")).toHaveCount(1);
      await expect(rich.locator("pre code")).toHaveCount(1);
      // KaTeX's stylesheet loads lazily; until it applies, the MathML copy shows as text.
      await expect
        .poll(() => rich.locator(".katex-mathml").first().evaluate((element) => getComputedStyle(element).position))
        .toBe("absolute");
      await page.locator('[data-testid="timeline-view"]').evaluate((element) => {
        element.scrollTop = element.scrollHeight;
      });
      await capture(page, locale, "rich", page.locator(".timeline-pane, main").first());
    });

    test("notifications", async ({ page }) => {
      await openWorkspace(page, locale, {
        mutateSnapshot: (snapshot) => {
          snapshot.state.domain.settings.values.delivery = { sinks: SYNTHETIC_SINKS };
          snapshot.state.domain.notification_delivery = {
            ...snapshot.state.domain.notification_delivery,
            sinks: SYNTHETIC_SINKS.map((sink) => ({
              sink_id: sink.id,
              secret_configured: true,
              secret_retired: false,
              last_outcome: null
            }))
          };
        }
      });
      await page.evaluate(() => window.__harness.pushDesktopMenu("openAccountSettings"));
      const dialog = page.getByRole("dialog", { name: label(locale, "settings.accountSettings") });
      await expect(dialog).toBeVisible();
      await dialog.locator("#settings-tab-notifications").click();
      await dialog.getByRole("tab", { name: label(locale, "settings.notificationDelivery"), exact: true }).click();
      await expect(dialog.getByText(label(locale, "settings.notificationDeliveryConfigured"), { exact: false })).toHaveCount(3);
      await capture(page, locale, "notifications", dialog);
    });

    test("settings", async ({ page }) => {
      await openWorkspace(page, locale);
      await page.evaluate(() => window.__harness.pushDesktopMenu("openAccountSettings"));
      const dialog = page.getByRole("dialog", { name: label(locale, "settings.accountSettings") });
      await expect(dialog).toBeVisible();
      await capture(page, locale, "settings", dialog);
    });

    test("languages", async ({ page }) => {
      await openWorkspace(page, locale);
      await page.evaluate(() => window.__harness.pushDesktopMenu("openAppSettings"));
      const dialog = page.getByRole("dialog", { name: label(locale, "settings.appSettings") });
      await dialog.locator("#settings-tab-appearance").click();
      await expect(dialog.getByText(label(locale, "settings.language"), { exact: true }).first()).toBeVisible();
      await capture(page, locale, "languages", dialog);
    });

    for (const scene of ["search-mixed", "search-width"] as const) {
      test(scene, async ({ page }) => {
        const { query, results } = SEARCH_SCENES[scene];
        const names = SITE_TEXT[locale].searchRooms;
        await openWorkspace(page, locale, {
          mutateSnapshot: (snapshot) => {
            snapshot.state.domain.rooms = [
              ...snapshot.state.domain.rooms,
              searchRoom("!site-ja:example.invalid", names.ja),
              searchRoom("!site-en:example.invalid", names.en),
              searchRoom("!site-mixed:example.invalid", names.mixed)
            ];
          }
        });
        await page.evaluate(
          ({ query, results }) => {
            window.__harness.setCommandResponse("submit_search", () => {
              const next = window.__harness.currentSnapshot();
              return {
                ...next,
                state: {
                  ...next.state,
                  domain: {
                    ...next.state.domain,
                    search: { kind: "results", request_id: 1, query, scope: "allRooms", results }
                  }
                }
              };
            });
          },
          { query, results: results.map(toFrontendSearchResult) }
        );
        await page.locator("select.scope-select").selectOption("allRooms");
        await page.locator(".top-search input").fill(query);
        const panel = page.locator(".thread-pane.search-panel");
        await expect(panel.locator(".result-button")).toHaveCount(results.length);
        for (const result of results) {
          await expect(panel.getByText(result.snippet.slice(0, 12), { exact: false }).first()).toBeVisible();
        }
        await capture(page, locale, scene, page.locator(".app-grid"));
      });
    }
  });
}
