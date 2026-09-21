import { describe, expect, it } from "vitest";
import { cachedDateTimeFormat, cachedListFormat } from "./intlFormatCache";

describe("intlFormatCache", () => {
  it("reuses one DateTimeFormat per locale and options", () => {
    const first = cachedDateTimeFormat("en", { timeStyle: "short" });
    expect(cachedDateTimeFormat("en", { timeStyle: "short" })).toBe(first);
    expect(cachedDateTimeFormat("ja", { timeStyle: "short" })).not.toBe(first);
    expect(cachedDateTimeFormat("en", { dateStyle: "medium" })).not.toBe(first);
    expect(cachedDateTimeFormat(undefined, { timeStyle: "short" })).not.toBe(first);
  });

  it("formats exactly like a fresh formatter", () => {
    const options = { weekday: "short", year: "numeric", month: "short", day: "numeric" } as const;
    const date = new Date(Date.UTC(2026, 8, 21, 12, 0, 0));
    for (const locale of ["en", "ja"]) {
      expect(cachedDateTimeFormat(locale, options).format(date)).toBe(
        new Intl.DateTimeFormat(locale, options).format(date)
      );
    }
  });

  it("reuses one ListFormat per locale and options", () => {
    const options = { style: "long", type: "conjunction" } as const;
    const first = cachedListFormat("en", options);
    expect(cachedListFormat("en", options)).toBe(first);
    expect(cachedListFormat("ja", options)).not.toBe(first);
    expect(first.format(["a", "b", "c"])).toBe("a, b, and c");
  });
});
