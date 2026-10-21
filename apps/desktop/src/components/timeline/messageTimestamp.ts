import { uiDateTimeFormat } from "../../domain/intlFormatCache";
import { t } from "../../i18n/messages";

// Compare civil dates, not elapsed 24-hour periods: DST changes day length.
function calendarDay(date: Date): number {
  const civil = new Date(0);
  civil.setUTCFullYear(date.getFullYear(), date.getMonth(), date.getDate());
  civil.setUTCHours(0, 0, 0, 0);
  return civil.getTime() / 86_400_000;
}

export type MessageTimestampStyle = "relative" | "posted" | "activity";

export function formatMessageTimestamp(timestampMs: number | null, nowMs = Date.now(), style: MessageTimestampStyle = "relative"): string | null {
  if (timestampMs === null) return null;
  const date = new Date(timestampMs);
  const now = new Date(nowMs);
  const daysAgo = calendarDay(now) - calendarDay(date);
  const time = uiDateTimeFormat({ timeStyle: "short" }).format(date);
  if (style === "posted") {
    return `${uiDateTimeFormat({
      weekday: "short", month: "short", day: "numeric",
      ...(date.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {})
    }).format(date)} ${time}`;
  }
  if (daysAgo === 0) return style === "activity" ? `${t("timeline.timestampToday")} ${time}` : time;
  if (daysAgo === 1) return `${t("timeline.timestampYesterday")} ${time}`;
  const options: Intl.DateTimeFormatOptions = daysAgo > 1 && daysAgo < 7
    ? { weekday: "long" }
    : date.getFullYear() === now.getFullYear()
      ? { month: "short", day: "numeric" }
      : { year: "numeric", month: "short", day: "numeric" };
  return `${uiDateTimeFormat(options).format(date)} ${time}`;
}

export function formatFullMessageTimestamp(timestampMs: number): string {
  return uiDateTimeFormat({ dateStyle: "full", timeStyle: "long" }).format(new Date(timestampMs));
}
