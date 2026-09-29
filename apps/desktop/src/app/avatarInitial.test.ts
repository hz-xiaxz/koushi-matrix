import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { avatarInitial } from "./uiShared";

// #1055: one Element/Compound single-grapheme rule (#414) for every
// placeholder avatar. Keep these cases in sync with the Rust receipt-reader
// initials test in crates/koushi-core/src/timeline/receipt_endpoints/tests.rs.
const cases: Array<[string, string]> = [
  ["Firstname Lastname", "F"],
  ["alice", "A"],
  ["Émile Zola", "É"],
  ["émile", "É"],
  ["😀 Smile", "😀"],
  ["👩‍👩‍👧 Family", "👩‍👩‍👧"],
  ["日本語", "日"],
  ["علي", "ع"],
  ["@alice:example.invalid", "A"],
  ["#general:example.invalid", "G"],
  ["+community:example.invalid", "C"],
  ["  Padded Name", "P"],
  ["", "?"],
  ["   ", "?"]
];

describe("avatarInitial", () => {
  test.each(cases)("%j renders %j", (name, expected) => {
    expect(avatarInitial(name)).toBe(expected);
  });

  test("tolerates a missing name", () => {
    expect(avatarInitial(null)).toBe("?");
    expect(avatarInitial(undefined)).toBe("?");
  });

  test("placeholder-avatar initials have one owner", () => {
    const root = new URL("../", import.meta.url);
    const offenders = (readdirSync(root, { recursive: true }) as string[])
      .filter((path) => /\.tsx?$/.test(path) && !/\.test\.tsx?$/.test(path))
      .filter((path) => {
        const source = readFileSync(new URL(path, root), "utf8");
        return (
          /function \w*[Ii]nitials?\s*\(/.test(source) ||
          /\[A-Za-z\]\/g/.test(source) ||
          /charAt\(0\)\.toUpperCase\(\)/.test(source)
        );
      })
      .filter((path) => !path.endsWith("app/uiShared.ts"));

    expect(offenders).toEqual([]);
  });
});
