// @vitest-environment jsdom
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import * as migration from "./legacyPreferenceMigration";
import { LEGACY_PREFERENCE_KEYS } from "./legacyPreferenceMigration";
import { useLegacyEmojiVocabulary } from "./useLegacyEmojiVocabulary";

describe("useLegacyEmojiVocabulary (#1035)", () => {
  afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.restoreAllMocks();
  });

  test("profiles without a legacy recent-emoji list never wait for the emoji chunk", () => {
    const { result } = renderHook(() => useLegacyEmojiVocabulary());
    expect(result.current.kind).toBe("ready");
    expect(result.current.kind === "ready" && result.current.emojis.size).toBe(0);
  });

  test("a legacy list holds the migration until the full vocabulary has loaded", async () => {
    localStorage.setItem(LEGACY_PREFERENCE_KEYS.recentEmojis, JSON.stringify(["😀"]));
    const { result } = renderHook(() => useLegacyEmojiVocabulary());
    // Running the migration now would validate against an empty vocabulary
    // and drop the legacy emojis, so the hook reports "loading".
    expect(result.current.kind).toBe("loading");
    await waitFor(() => expect(result.current.kind).toBe("ready"));
    const current = result.current;
    expect(current.kind === "ready" && current.emojis.has("😀")).toBe(true);
    expect(current.kind === "ready" && current.emojis.size).toBeGreaterThan(1000);
  });

  test("a failed chunk load reports the vocabulary as unavailable", async () => {
    localStorage.setItem(LEGACY_PREFERENCE_KEYS.recentEmojis, JSON.stringify(["😀"]));
    vi.spyOn(migration, "loadLegacyEmojiVocabulary").mockRejectedValue(
      new Error("chunk failed")
    );
    const { result } = renderHook(() => useLegacyEmojiVocabulary());
    await waitFor(() => expect(result.current.kind).toBe("unavailable"));
  });
});
