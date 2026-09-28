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

// The picker module, its dataset module, or the emojibase package itself.
const PICKER_SPECIFIER =
  String.raw`["'](?:[^"']*\/(?:EmojiPicker|emojiData)|@matrix-org\/emojibase-bindings(?:\/[^"']*)?)["']`;
// Value imports and re-exports (`import x from`, `import { a, type b } from`,
// `export { x } from`, `export * from`) plus side-effect imports
// (`import "…"`). Type-only imports and exports are erased and allowed.
const STATIC_PICKER_IMPORTS = [
  new RegExp(String.raw`^\s*import\s+(?!type\b)[^;]*?from\s+${PICKER_SPECIFIER}`, "m"),
  new RegExp(String.raw`^\s*export\s+(?!type\b)[^;]*?from\s+${PICKER_SPECIFIER}`, "m"),
  new RegExp(String.raw`^\s*import\s+${PICKER_SPECIFIER}`, "m")
];

function staticallyImportsPicker(source: string): boolean {
  return STATIC_PICKER_IMPORTS.some((pattern) => pattern.test(source));
}

describe("emoji picker loading boundary (#1035)", () => {
  test("no production module statically imports the picker or its dataset", () => {
    const offenders = productionSources(srcRoot)
      .map((path) => relative(srcRoot, path).split("\\").join("/"))
      .filter((path) => !PICKER_CHUNK_MODULES.has(path))
      .filter((path) => staticallyImportsPicker(readFileSync(join(srcRoot, path), "utf8")));
    expect(offenders).toEqual([]);
  });

  test("the scan recognizes every static form that would defeat the boundary", () => {
    for (const offending of [
      'import { EmojiPicker } from "./EmojiPicker";',
      'import { type Emoji, EMOJI_CATEGORIES } from "../components/emojiData";',
      'export { EmojiPicker } from "./EmojiPicker";',
      'export * from "./emojiData";',
      'import "./EmojiPicker";',
      'import { getEmojiData } from "@matrix-org/emojibase-bindings";',
      'export { default } from "@matrix-org/emojibase-bindings/data";',
      'import "@matrix-org/emojibase-bindings";',
      'import {\n  EMOJI_BY_CATEGORY\n} from "./emojiData";'
    ]) {
      expect(staticallyImportsPicker(offending), offending).toBe(true);
    }
    for (const allowed of [
      'import type { EmojiPicker } from "./EmojiPicker";',
      'export type { Emoji } from "./emojiData";',
      'import { LazyEmojiPicker } from "./LazyEmojiPicker";',
      'const chunk = import("./EmojiPicker");'
    ]) {
      expect(staticallyImportsPicker(allowed), allowed).toBe(false);
    }
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
