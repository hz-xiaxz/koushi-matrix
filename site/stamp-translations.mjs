#!/usr/bin/env node
// Records that a locale's text is a translation of the current English text.
// Usage: node site/stamp-translations.mjs <locale> <key...|--all> [--from FILE]
//   --from FILE  plain {key: text} JSON whose texts replace the listed keys first.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { hashText, LOCALES } from "./lib/content.mjs";

const contentDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "content");
const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { all: { type: "boolean", default: false }, from: { type: "string" } }
});
const [locale, ...keys] = positionals;
if (!LOCALES.includes(locale) || locale === "en" || (!values.all && keys.length === 0)) {
  console.error("Usage: node site/stamp-translations.mjs <locale> <key...|--all> [--from FILE]");
  process.exit(2);
}
const en = JSON.parse(readFileSync(path.join(contentDir, "en.json"), "utf8"));
const file = path.join(contentDir, `${locale}.json`);
let current = {};
try {
  current = JSON.parse(readFileSync(file, "utf8"));
} catch {
  // first stamp creates the file
}
const incoming = values.from ? JSON.parse(readFileSync(values.from, "utf8")) : {};
const targets = values.all ? Object.keys(en) : keys;
for (const key of targets) {
  if (!(key in en)) {
    console.error(`unknown key ${key}`);
    process.exit(1);
  }
  const text = incoming[key] ?? current[key]?.text;
  if (typeof text !== "string") {
    console.error(`${locale}: no text for ${key}`);
    process.exit(1);
  }
  current[key] = { text, source_hash: hashText(en[key]) };
}
const ordered = Object.fromEntries(Object.keys(en).filter((key) => key in current).map((key) => [key, current[key]]));
writeFileSync(file, `${JSON.stringify(ordered, null, 2)}\n`);
console.log(`${locale}: stamped ${targets.length} key(s)`);
