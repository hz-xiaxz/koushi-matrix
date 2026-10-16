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
    for (const locale of ["en", "ja", "zh-Hans", "zh-Hant", "ko"]) {
      expect(cachedDateTimeFormat(locale, options).format(date)).toBe(
        new Intl.DateTimeFormat(locale, options).format(date)
      );
    }
  });

  it("catalog locale tags select distinct CJK date and list formatting (#1461)", () => {
    const date = new Date(Date.UTC(2026, 8, 21, 12, 0, 0));
    const options = { year: "numeric", month: "long", day: "numeric", timeZone: "UTC" } as const;
    expect(cachedDateTimeFormat("zh-Hans", options).format(date)).toBe("2026年9月21日");
    expect(cachedDateTimeFormat("zh-Hant", options).format(date)).toBe("2026年9月21日");
    expect(cachedDateTimeFormat("ko", options).format(date)).toBe("2026년 9월 21일");
    const list = { style: "long", type: "conjunction" } as const;
    expect(cachedListFormat("zh-Hans", list).format(["甲", "乙", "丙"])).toBe("甲、乙和丙");
    expect(cachedListFormat("zh-Hant", list).format(["甲", "乙", "丙"])).toBe("甲、乙和丙");
    expect(cachedListFormat("ko", list).format(["가", "나", "다"])).toBe("가, 나 및 다");
  });

  it("reuses one ListFormat per locale and options", () => {
    const options = { style: "long", type: "conjunction" } as const;
    const first = cachedListFormat("en", options);
    expect(cachedListFormat("en", options)).toBe(first);
    expect(cachedListFormat("ja", options)).not.toBe(first);
    expect(first.format(["a", "b", "c"])).toBe("a, b, and c");
  });
});

describe("clearIntlFormatCache", () => {
  it("drops cached formatters so a changed system time zone is picked up", async () => {
    const { clearIntlFormatCache } = await import("./intlFormatCache");
    const before = cachedDateTimeFormat("en", { timeStyle: "short" });
    clearIntlFormatCache();
    expect(cachedDateTimeFormat("en", { timeStyle: "short" })).not.toBe(before);
  });
});
