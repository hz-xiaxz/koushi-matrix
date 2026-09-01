import { describe, expect, test } from "vitest";

import { readLegacyPreferenceMigration } from "./legacyPreferenceMigration";

const currentSettings = {
  appearance: { theme: "dark" as const, density: "comfortable" as const },
  composer: { math_mode: false, recent_emojis: [] as string[] },
  sidebar: {
    category: "rooms" as const,
    collapsed: { favourites: false, low_priority: false, not_joined: false }
  }
};

function memoryStorage(entries: Record<string, string>): Storage {
  const values = new Map(Object.entries(entries));
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => {
      values.delete(key);
    },
    setItem: (key, value) => {
      values.set(key, value);
    }
  };
}

describe("legacy frontend preference migration", () => {
  test("strictly maps all seven legacy key families without mutating storage", () => {
    const storage = memoryStorage({
      "koushi.spaceLocalOverrides.v1": JSON.stringify({
        "!space:example.invalid": { name: "  Local Space  ", icon: "🧪" }
      }),
      "koushi.displayDensity.v1": "compact",
      "koushi.sidebarRoomCategory.v1": "dms",
      "koushi.sidebarRoomSort.v1": "name",
      "koushi.homeSelection.v1": JSON.stringify({
        kind: "dm",
        roomId: "!dm:example.invalid"
      }),
      "koushi.roomSectionCollapsed.v1": JSON.stringify({
        favourites: true,
        "low-priority": true,
        unexpected: true
      }),
      "koushi-recent-emojis": JSON.stringify(["😀", "😀", "🚀", "not-an-emoji"])
    });

    const migration = readLegacyPreferenceMigration(
      storage,
      new Set(["😀", "🚀"]),
      currentSettings
    );

    expect(migration.settingsPatch).toMatchObject({
      appearance: { theme: "dark", density: "compact" },
      sidebar: {
        category: "people",
        collapsed: { favourites: true, low_priority: true, not_joined: false }
      },
      room_list_sort: { kind: "normalLocale" },
      composer: { math_mode: false, recent_emojis: ["😀", "🚀"] }
    });
    expect(migration.navigationImport).toEqual({
      kind: "importLegacy",
      home_selection: { kind: "directMessage", room_id: "!dm:example.invalid" },
      space_local_presentations: {
        "!space:example.invalid": { name: "Local Space", icon: "🧪" }
      }
    });
    expect(migration.sourceKeys).toHaveLength(7);
    expect(storage.length).toBe(7);
  });

  test("ignores corrupt, unknown and oversized values without inventing defaults", () => {
    const storage = memoryStorage({
      "koushi.spaceLocalOverrides.v1": "{broken",
      "koushi.displayDensity.v1": "dense",
      "koushi.sidebarRoomCategory.v1": "all",
      "koushi.sidebarRoomSort.v1": "random",
      "koushi.homeSelection.v1": JSON.stringify({ kind: "dm", roomId: 12 }),
      "koushi.roomSectionCollapsed.v1": JSON.stringify(["favourites"]),
      "koushi-recent-emojis": JSON.stringify(["x".repeat(200)])
    });

    const migration = readLegacyPreferenceMigration(storage, new Set(["😀"]), currentSettings);
    expect(migration.settingsPatch).toEqual({});
    expect(migration.navigationImport).toBeNull();
    expect(migration.sourceKeys).toEqual([]);
    expect(storage.length).toBe(7);
  });

  test("returns only present valid fields so existing Rust values are not reset", () => {
    const storage = memoryStorage({
      "koushi.displayDensity.v1": "comfortable",
      "koushi-recent-emojis": JSON.stringify(["🚀"])
    });

    expect(
      readLegacyPreferenceMigration(storage, new Set(["🚀"]), currentSettings)
    ).toEqual({
      settingsPatch: {
        appearance: { theme: "dark", density: "comfortable" },
        composer: { math_mode: false, recent_emojis: ["🚀"] }
      },
      navigationImport: null,
      sourceKeys: ["koushi.displayDensity.v1", "koushi-recent-emojis"]
    });
  });
});
