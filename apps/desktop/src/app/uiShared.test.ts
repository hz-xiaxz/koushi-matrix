import { afterEach, describe, expect, it } from "vitest";

import { setActiveLocaleProfile, t } from "../i18n/messages";
import {
  SCHEDULED_SEND_PERSISTENCE_FAILED_CODE,
  formatScheduledSendTime,
  formatTime,
  graphemeCount,
  hasScheduledSendPersistenceFailure,
  roomAccessTooltipLabel
} from "./uiShared";

describe("scheduled-send persistence failure (#1159)", () => {
  it("matches only Core's local-save failure code", () => {
    expect(
      hasScheduledSendPersistenceFailure([
        { code: SCHEDULED_SEND_PERSISTENCE_FAILED_CODE }
      ])
    ).toBe(true);
    expect(hasScheduledSendPersistenceFailure([{ code: "other_failure" }])).toBe(false);
    expect(hasScheduledSendPersistenceFailure([])).toBe(false);
  });

  it("uses the code Core publishes", () => {
    // Mirrors `SCHEDULED_SEND_PERSISTENCE_FAILED` in
    // crates/koushi-state/src/reducer/timeline.rs.
    expect(SCHEDULED_SEND_PERSISTENCE_FAILED_CODE).toBe(
      "scheduled_send_persistence_failed"
    );
  });
});

describe("grapheme counting (#1217)", () => {
  it("counts a decomposed name as the same length as its precomposed form", () => {
    expect(graphemeCount("é")).toBe(1);
    expect(graphemeCount("e\u0301")).toBe(1);
    expect(graphemeCount("café")).toBe(4);
    expect(graphemeCount("cafe\u0301")).toBe(4);
  });

  it("counts CJK and emoji sequences as single graphemes", () => {
    expect(graphemeCount("研究室")).toBe(3);
    expect(graphemeCount("👩‍👩‍👧")).toBe(1);
  });
});

describe("room access tooltip label (#1166)", () => {
  it("lists a restricted rule's named routes only when they were resolved", () => {
    expect(
      roomAccessTooltipLabel("access.conditionsDescription", ["Alpha Room", "Beta Space"])
    ).toBe(
      `${t("access.conditionsDescription")} ${t("access.allowedRooms", {
        rooms: `Alpha Room${t("access.labelSeparator")}Beta Space`
      })}`
    );
  });

  it("falls back to the plain explanation when nothing resolved", () => {
    // No resolved names: never a raw id and never a guessed label.
    expect(roomAccessTooltipLabel("access.conditionsDescription", [])).toBe(
      t("access.conditionsDescription")
    );
    expect(roomAccessTooltipLabel("access.conditionsDescription", ["   "])).toBe(
      t("access.conditionsDescription")
    );
    expect(roomAccessTooltipLabel("access.conditionsDescription", null)).toBe(
      t("access.conditionsDescription")
    );
    expect(roomAccessTooltipLabel("access.conditionsDescription")).toBe(
      t("access.conditionsDescription")
    );
  });
});

describe("date/time formatting follows the selected catalog locale (#1477)", () => {
  afterEach(() => setActiveLocaleProfile("en"));

  const timestampMs = Date.UTC(2026, 8, 21, 14, 5, 0);
  const timeOptions = { hour: "2-digit", minute: "2-digit" } as const;
  const scheduledOptions = { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" } as const;

  it.each(["en", "ja", "zh-Hans", "ko"] as const)("formats under the %s catalog", (locale) => {
    setActiveLocaleProfile(locale);
    expect(formatTime(timestampMs)).toBe(
      new Intl.DateTimeFormat(locale, timeOptions).format(timestampMs)
    );
    expect(formatScheduledSendTime(timestampMs)).toBe(
      new Intl.DateTimeFormat(locale, scheduledOptions).format(timestampMs)
    );
  });

  it("renders English and Japanese times differently", () => {
    setActiveLocaleProfile("en");
    const english = formatTime(timestampMs);
    setActiveLocaleProfile("ja");
    expect(formatTime(timestampMs)).not.toBe(english);
  });

  it("formats the pseudo catalog as English", () => {
    setActiveLocaleProfile("pseudo", "accented");
    expect(formatTime(timestampMs)).toBe(
      new Intl.DateTimeFormat("en", timeOptions).format(timestampMs)
    );
  });
});
