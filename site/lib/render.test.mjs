import assert from "node:assert/strict";
import test from "node:test";

import { renderPage, SCENES } from "./render.mjs";
import { LOCALES } from "./content.mjs";
import { readFileSync } from "node:fs";

const en = JSON.parse(readFileSync(new URL("../content/en.json", import.meta.url), "utf8"));
const release = { tag: "v0.26.0", version: "0.26.0" };
const strings = (overrides = {}) => ({ ...en, ...overrides });

test("scene ids match the screenshot lane", () => {
  assert.deepEqual(SCENES, ["main", "notifications", "search-mixed", "search-width", "settings", "languages", "rich"]);
});

test("each page declares its language and five hreflang alternates", () => {
  for (const locale of LOCALES) {
    const html = renderPage({ locale, strings: strings(), staleKeys: [], release });
    assert.match(html, new RegExp(`<html lang="${locale}"`));
    assert.equal((html.match(/rel="alternate" hreflang=/g) ?? []).length, 6, locale); // five + x-default
  }
});

test("no link or source is root-absolute, so the site works under /koushi-matrix/", () => {
  for (const locale of LOCALES) {
    const html = renderPage({ locale, strings: strings(), staleKeys: [], release });
    assert.doesNotMatch(html, /(?:href|src)="\/(?!\/)/, locale);
  }
});

test("help links point at the release tag", () => {
  const html = renderPage({ locale: "en", strings: strings(), staleKeys: [], release });
  assert.match(html, /\/blob\/v0\.26\.0\/docs\/help\/README\.md/);
  assert.match(html, /Version 0\.26\.0/);
});

test("non-root pages reach shared assets one level up", () => {
  const html = renderPage({ locale: "ja", strings: strings(), staleKeys: [], release });
  assert.match(html, /src="\.\.\/screenshots\/ja\/main\.png"/);
  assert.match(html, /href="\.\.\/style\.css"/);
  const root = renderPage({ locale: "en", strings: strings(), staleKeys: [], release });
  assert.match(root, /src="screenshots\/en\/main\.png"/);
});

test("renders English and the translating note for a stale key", () => {
  const html = renderPage({
    locale: "ja",
    strings: strings({ "hero.tagline": "多言語の会話のためのデスクトップ Matrix クライアント。" }),
    staleKeys: ["hero.lede"],
    englishStrings: en,
    release
  });
  assert.match(html, /多言語の会話/);
  assert.ok(html.includes(en["hero.lede"]));
  assert.ok(html.includes(en["translating.note"]));
  const clean = renderPage({ locale: "ja", strings: strings(), staleKeys: [], englishStrings: en, release });
  assert.ok(!clean.includes('class="translating-note"'));
});

test("japanese pages link the Japanese blog posts; others the English ones", () => {
  const ja = renderPage({ locale: "ja", strings: strings(), staleKeys: [], release });
  assert.match(ja, /blog\/naze-matrix-ja\//);
  assert.match(ja, /blog\/matrix-encryption-setup-checklist-ja\//);
  const ko = renderPage({ locale: "ko", strings: strings(), staleKeys: [], release });
  assert.match(ko, /blog\/why-matrix\//);
});

test("text is HTML-escaped", () => {
  const html = renderPage({ locale: "en", strings: strings({ "hero.tagline": "<b>&" }), staleKeys: [], release });
  assert.ok(html.includes("&lt;b&gt;&amp;"));
});

test("a missing string throws instead of rendering a blank", () => {
  const partial = { ...en };
  delete partial["pain.utd.answer"];
  assert.throws(() => renderPage({ locale: "en", strings: partial, staleKeys: [], release }), /pain\.utd\.answer/);
});

test("the header and hero link prominently to the GitHub repository", () => {
  const html = renderPage({ locale: "ja", strings: strings(), staleKeys: [], release });
  assert.match(html, /<a class="github-link" href="https:\/\/github\.com\/shinaoka\/koushi-matrix"/);
  assert.match(html, /<a class="button secondary" href="https:\/\/github\.com\/shinaoka\/koushi-matrix">/);
  assert.ok(html.includes(en["hero.github"]));
});

test("the AI assistant section links its setup guide at the release tag", () => {
  const html = renderPage({ locale: "en", strings: strings(), staleKeys: [], release });
  assert.match(html, /<section class="ai" id="ai">/);
  assert.match(html, /\/blob\/v0\.26\.0\/docs\/help\/ai-assistants\.md/);
  assert.ok(html.includes("claude mcp add --scope user koushi"));
});
