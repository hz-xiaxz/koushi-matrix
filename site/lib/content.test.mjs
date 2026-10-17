import assert from "node:assert/strict";
import test from "node:test";

import { checkTranslations, hashText, LOCALES } from "./content.mjs";

const en = { "hero.title": "Hello", "hero.body": "World" };
const entry = (key, text) => ({ text, source_hash: hashText(en[key]) });

test("locales are the five shipped interface languages", () => {
  assert.deepEqual(LOCALES, ["en", "ja", "zh-Hans", "zh-Hant", "ko"]);
});

test("a missing or extra key is an error naming locale and key", () => {
  const { errors } = checkTranslations(en, {
    ja: { "hero.body": entry("hero.body", "世界"), "hero.extra": { text: "x", source_hash: "0" } }
  });
  assert.deepEqual(errors.sort(), ["ja: extra key hero.extra", "ja: missing key hero.title"]);
});

test("a stale source hash is a warning, not an error", () => {
  const { errors, stale } = checkTranslations(en, {
    ja: {
      "hero.title": { text: "こんにちは", source_hash: hashText("Hi") },
      "hero.body": entry("hero.body", "世界")
    }
  });
  assert.deepEqual(errors, []);
  assert.deepEqual(stale, { ja: ["hero.title"] });
});

test("a malformed entry is an error", () => {
  const { errors } = checkTranslations(en, {
    ko: { "hero.title": "plain string", "hero.body": entry("hero.body", "세계") }
  });
  assert.deepEqual(errors, ["ko: hero.title must be {text, source_hash}"]);
});

test("hashText is a stable 12-hex prefix of SHA-256", () => {
  assert.equal(hashText("Hello"), "185f8db32271");
});
