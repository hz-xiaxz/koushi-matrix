import { afterEach, describe, expect, it } from "vitest";
import { uiDateTimeFormat } from "../../domain/intlFormatCache";
import { setActiveLocaleProfile, t, type Locale } from "../../i18n/messages";
import { formatFullMessageTimestamp, formatMessageTimestamp } from "./messageTimestamp";

afterEach(() => setActiveLocaleProfile("en"));

const now = new Date(2026, 9, 21, 12).getTime();
const time = (date: Date) => uiDateTimeFormat({ timeStyle: "short" }).format(date);

describe("message timestamp calendar context", () => {
  it.each<Locale>(["en", "ja", "ko", "zh-Hans", "zh-Hant", "pseudo"])("formats categories in the active %s catalog", (locale) => {
    setActiveLocaleProfile(locale);
    const today = new Date(2026, 9, 21, 9, 30);
    expect(formatMessageTimestamp(today.getTime(), now)).toBe(time(today));
    const yesterday = new Date(2026, 9, 20, 9, 30);
    expect(formatMessageTimestamp(yesterday.getTime(), now)).toBe(`${t("timeline.timestampYesterday")} ${time(yesterday)}`);
    const recent = new Date(2026, 9, 15, 9, 30);
    expect(formatMessageTimestamp(recent.getTime(), now)).toBe(`${uiDateTimeFormat({ weekday: "long" }).format(recent)} ${time(recent)}`);
    for (const date of [new Date(2026, 9, 14, 9, 30), new Date(2026, 0, 1, 9, 30)]) {
      expect(formatMessageTimestamp(date.getTime(), now)).toBe(`${uiDateTimeFormat({ month: "short", day: "numeric" }).format(date)} ${time(date)}`);
    }
    const old = new Date(2025, 11, 31, 9, 30);
    expect(formatMessageTimestamp(old.getTime(), now)).toBe(`${uiDateTimeFormat({ year: "numeric", month: "short", day: "numeric" }).format(old)} ${time(old)}`);
    expect(formatFullMessageTimestamp(old.getTime())).toBe(uiDateTimeFormat({ dateStyle: "full", timeStyle: "long" }).format(old));
  });

  it("uses civil days over spring and fall DST boundaries", () => {
    // The focused gate runs with TZ=America/New_York; these intervals are 23/25 hours.
    for (const [month, day] of [[2, 9], [10, 2]]) {
      const today = new Date(2026, month, day, 0, 30);
      const yesterday = new Date(2026, month, day - 1, 0, 30);
      expect(formatMessageTimestamp(yesterday.getTime(), today.getTime())).toBe(`Yesterday ${time(yesterday)}`);
      const twoDaysAgo = new Date(2026, month, day - 2, 23, 59);
      expect(formatMessageTimestamp(twoDaysAgo.getTime(), today.getTime())).toBe(`${uiDateTimeFormat({ weekday: "long" }).format(twoDaysAgo)} ${time(twoDaysAgo)}`);
    }
  });

  it("handles yesterday across a year boundary and future dates without relative past labels", () => {
    const yesterday = new Date(2025, 11, 31, 23, 59);
    expect(formatMessageTimestamp(yesterday.getTime(), new Date(2026, 0, 1, 0, 1).getTime())).toBe(`Yesterday ${time(yesterday)}`);
    const future = new Date(2026, 9, 22, 9);
    expect(formatMessageTimestamp(future.getTime(), now)).toBe(`${uiDateTimeFormat({ month: "short", day: "numeric" }).format(future)} ${time(future)}`);
    const futureYear = new Date(2027, 0, 1, 9);
    expect(formatMessageTimestamp(futureYear.getTime(), now)).toContain("2027");
    expect(formatMessageTimestamp(null, now)).toBeNull();
  });
});
