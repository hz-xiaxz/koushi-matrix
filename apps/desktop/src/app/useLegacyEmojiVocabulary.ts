import { useEffect, useState } from "react";

import {
  browserHasLegacyRecentEmojis,
  loadLegacyEmojiVocabulary
} from "./legacyPreferenceMigration";

const NO_LEGACY_RECENT_EMOJIS: ReadonlySet<string> = new Set();

export type LegacyEmojiVocabulary =
  | { kind: "loading" }
  | { kind: "ready"; emojis: ReadonlySet<string> }
  | { kind: "unavailable" };

/**
 * Emoji vocabulary for the one-time legacy preference migration.
 *
 * `loading` holds only the settings import: running it with an empty
 * vocabulary would drop every legacy recent emoji. Profiles without a legacy
 * list never consult the vocabulary, so they are `ready` immediately and no
 * chunk is fetched. If the chunk fails to load the vocabulary is
 * `unavailable`: the other legacy preferences still migrate, and the legacy
 * recent-emoji list, which cannot be validated, is left out of the import.
 * Navigation preferences never wait for the vocabulary.
 */
export function useLegacyEmojiVocabulary(): LegacyEmojiVocabulary {
  const [vocabulary, setVocabulary] = useState<LegacyEmojiVocabulary>(() =>
    browserHasLegacyRecentEmojis()
      ? { kind: "loading" }
      : { kind: "ready", emojis: NO_LEGACY_RECENT_EMOJIS }
  );
  const loading = vocabulary.kind === "loading";
  useEffect(() => {
    if (!loading) return;
    let active = true;
    loadLegacyEmojiVocabulary().then(
      (emojis) => {
        if (active) setVocabulary({ kind: "ready", emojis });
      },
      () => {
        if (active) setVocabulary({ kind: "unavailable" });
      }
    );
    return () => {
      active = false;
    };
  }, [loading]);
  return vocabulary;
}
