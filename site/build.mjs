#!/usr/bin/env node
// Usage: node site/build.mjs --tag vX.Y.Z [--screenshots DIR] [--out DIR]
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { buildSite } from "./lib/build.mjs";

const siteRoot = path.dirname(fileURLToPath(import.meta.url));
const { values } = parseArgs({
  options: {
    tag: { type: "string" },
    screenshots: { type: "string", default: path.join(siteRoot, "screenshots") },
    out: { type: "string", default: path.join(siteRoot, "dist") }
  }
});
if (!values.tag || !/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/.test(values.tag)) {
  console.error("Usage: node site/build.mjs --tag vX.Y.Z [--screenshots DIR] [--out DIR]");
  process.exit(2);
}
const { stale } = buildSite({
  contentDir: path.join(siteRoot, "content"),
  outDir: path.resolve(values.out),
  screenshotsDir: path.resolve(values.screenshots),
  release: { tag: values.tag, version: values.tag.slice(1) }
});
for (const [locale, keys] of Object.entries(stale)) {
  console.warn(`warning: ${locale} shows English for stale keys: ${keys.join(", ")}`);
}
console.log(`site built: ${path.resolve(values.out)}`);
