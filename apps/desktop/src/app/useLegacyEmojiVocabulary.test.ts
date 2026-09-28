// @vitest-environment jsdom
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";

import { LEGACY_PREFERENCE_KEYS } from "./legacyPreferenceMigration";
import { useLegacyEmojiVocabulary } from "./useLegacyEmojiVocabulary";

describe("useLegacyEmojiVocabulary (#1035)", () => {
  afterEach(() => {
    cleanup();
    localStorage.clear();
  });

  test("profiles without a legacy recent-emoji list never wait for the emoji chunk", () => {
    const { result } = renderHook(() => useLegacyEmojiVocabulary());
    expect(result.current).not.toBeNull();
    expect(result.current?.size).toBe(0);
  });

  test("a legacy list holds the migration until the full vocabulary has loaded", async () => {
    localStorage.setItem(LEGACY_PREFERENCE_KEYS.recentEmojis, JSON.stringify(["😀"]));
    const { result } = renderHook(() => useLegacyEmojiVocabulary());
    // Running the migration now would validate against an empty vocabulary
    // and drop the legacy emojis, so the hook reports "not ready".
    expect(result.current).toBeNull();
    await waitFor(() => expect(result.current?.has("😀")).toBe(true));
    expect(result.current?.size).toBeGreaterThan(1000);
  });
});
