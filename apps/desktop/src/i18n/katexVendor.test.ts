import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

const VENDORED = "../../crates/koushi-core/assets/katex";

describe("vendored KaTeX for exported history pages", () => {
  test("matches the npm dependency version", () => {
    const vendored = readFileSync(`${VENDORED}/VERSION`, "utf8").trim();
    const installed = JSON.parse(readFileSync("node_modules/katex/package.json", "utf8")).version;
    expect(vendored).toBe(installed);
  });

  test("CSS references only woff2 fonts that are vendored", () => {
    const css = readFileSync(`${VENDORED}/katex.min.css`, "utf8");
    const urls = [...css.matchAll(/url\(([^)]+)\)/g)].map((match) => match[1].replace(/["']/g, ""));
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) {
      expect(url.endsWith(".woff2")).toBe(true);
      expect(() => readFileSync(`${VENDORED}/${url}`)).not.toThrow();
    }
  });
});
