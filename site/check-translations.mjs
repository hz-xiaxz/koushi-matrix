#!/usr/bin/env node
// Fails on missing/extra/malformed translation keys; stale translations only warn.
import path from "node:path";
import { fileURLToPath } from "node:url";

import { checkTranslations, loadLocales } from "./lib/content.mjs";

const { en, locales } = loadLocales(path.join(path.dirname(fileURLToPath(import.meta.url)), "content"));
const { errors, stale } = checkTranslations(en, locales);
for (const [locale, keys] of Object.entries(stale)) {
  console.warn(`warning: ${locale} has stale translations (English will be shown): ${keys.join(", ")}`);
}
if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log("site translations ok");
