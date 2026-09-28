import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, test } from "vitest";

// #1035: the emoji picker and its emojibase dataset are the largest eager
// dependency of the startup bundle. Every production caller reaches them only
// through the on-demand boundary in `LazyEmojiPicker.tsx`; a single static
// import would pull the whole dataset back into the main chunk (Vite reports
// that as INEFFECTIVE_DYNAMIC_IMPORT), so this scan fails closed on it.
const srcRoot = fileURLToPath(new URL("..", import.meta.url));

// Modules that belong to the lazily loaded picker chunk itself.
const PICKER_CHUNK_MODULES = new Set([
  "components/EmojiPicker.tsx",
  "components/emojiData.ts"
]);

function productionSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return productionSources(path);
    if (!/\.(ts|tsx)$/.test(entry.name) || /\.test\.(ts|tsx)$/.test(entry.name)) return [];
    if (path.includes(`${join("src", "test")}`)) return [];
    return [path];
  });
}

const STATIC_PICKER_IMPORT =
  /^\s*import\s+(?!type\b)[^;]*?from\s+["'][^"']*\/(EmojiPicker|emojiData)["']/m;

describe("emoji picker loading boundary (#1035)", () => {
  test("no production module statically imports the picker or its dataset", () => {
    const offenders = productionSources(srcRoot)
      .map((path) => relative(srcRoot, path).split("\\").join("/"))
      .filter((path) => !PICKER_CHUNK_MODULES.has(path))
      .filter((path) => STATIC_PICKER_IMPORT.test(readFileSync(join(srcRoot, path), "utf8")));
    expect(offenders).toEqual([]);
  });

  test("the lazy boundary is the only dynamic importer of the picker module", () => {
    const dynamicImporters = productionSources(srcRoot)
      .map((path) => relative(srcRoot, path).split("\\").join("/"))
      .filter((path) =>
        /import\(\s*["'][^"']*\/EmojiPicker["']\s*\)/.test(readFileSync(join(srcRoot, path), "utf8"))
      );
    expect(dynamicImporters).toEqual(["components/LazyEmojiPicker.tsx"]);
  });
});
