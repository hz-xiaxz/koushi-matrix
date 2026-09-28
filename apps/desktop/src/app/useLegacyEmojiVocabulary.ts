import { useEffect, useState } from "react";

import {
  browserHasLegacyRecentEmojis,
  loadLegacyEmojiVocabulary
} from "./legacyPreferenceMigration";

const NO_LEGACY_RECENT_EMOJIS: ReadonlySet<string> = new Set();

/**
 * Emoji vocabulary for the one-time legacy preference migration.
 *
 * Returns `null` while the vocabulary is still loading. Callers must not run
 * the migration until it resolves: an import that ran with an empty vocabulary
 * would drop every legacy recent emoji. Profiles without a legacy list never
 * consult the vocabulary, so they get an empty set immediately and no chunk
 * is fetched. A failed chunk load keeps `null`, leaving the legacy key for a
 * later launch instead of discarding it.
 */
export function useLegacyEmojiVocabulary(): ReadonlySet<string> | null {
  const [vocabulary, setVocabulary] = useState<ReadonlySet<string> | null>(() =>
    browserHasLegacyRecentEmojis() ? null : NO_LEGACY_RECENT_EMOJIS
  );
  useEffect(() => {
    if (vocabulary) return;
    let active = true;
    loadLegacyEmojiVocabulary().then(
      (loaded) => {
        if (active) setVocabulary(loaded);
      },
      () => undefined
    );
    return () => {
      active = false;
    };
  }, [vocabulary]);
  return vocabulary;
}
