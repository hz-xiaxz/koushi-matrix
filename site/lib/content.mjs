// Website copy: English is the source of truth; each other locale records the hash
// of the English text it was translated from, so a stale translation is detectable.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

export const LOCALES = ["en", "ja", "zh-Hans", "zh-Hant", "ko"];

export function hashText(text) {
  return createHash("sha256").update(text, "utf8").digest("hex").slice(0, 12);
}

export function loadLocales(contentDir) {
  const read = (locale) => JSON.parse(readFileSync(path.join(contentDir, `${locale}.json`), "utf8"));
  const en = read("en");
  const locales = Object.fromEntries(LOCALES.filter((locale) => locale !== "en").map((locale) => [locale, read(locale)]));
  return { en, locales };
}

function placeholders(text) {
  return [...text.matchAll(/\{[a-z]+\}/g)].map((match) => match[0]).sort().join(" ");
}

export function checkTranslations(en, locales) {
  const errors = [];
  const stale = {};
  for (const [locale, entries] of Object.entries(locales)) {
    for (const key of Object.keys(en)) {
      if (!(key in entries)) {
        errors.push(`${locale}: missing key ${key}`);
        continue;
      }
      const entry = entries[key];
      if (typeof entry !== "object" || entry === null || typeof entry.text !== "string" || typeof entry.source_hash !== "string") {
        errors.push(`${locale}: ${key} must be {text, source_hash}`);
        continue;
      }
      const expected = placeholders(en[key]);
      if (placeholders(entry.text) !== expected) {
        errors.push(`${locale}: ${key} must keep placeholders ${expected}`);
        continue;
      }
      if (entry.source_hash !== hashText(en[key])) {
        (stale[locale] ??= []).push(key);
      }
    }
    for (const key of Object.keys(entries)) {
      if (!(key in en)) {
        errors.push(`${locale}: extra key ${key}`);
      }
    }
  }
  return { errors, stale };
}

/** Flat key → text for one locale; the renderer substitutes English for stale keys. */
export function resolveStrings(en, locales, locale) {
  if (locale === "en") {
    return { ...en };
  }
  return Object.fromEntries(Object.keys(en).map((key) => [key, locales[locale][key].text]));
}
