import type {
  AppearanceSettings,
  ComposerSettings,
  SettingsPatch,
  SettingsValues,
  SidebarSettings,
  NavigationState
} from "../domain/types";

export const LEGACY_PREFERENCE_KEYS = {
  spacePresentations: "koushi.spaceLocalOverrides.v1",
  density: "koushi.displayDensity.v1",
  sidebarCategory: "koushi.sidebarRoomCategory.v1",
  sidebarSort: "koushi.sidebarRoomSort.v1",
  homeSelection: "koushi.homeSelection.v1",
  collapsedSections: "koushi.roomSectionCollapsed.v1",
  recentEmojis: "koushi-recent-emojis"
} as const;

const MAX_SPACE_PRESENTATIONS = 256;
const MAX_SPACE_ID_SCALARS = 255;
const MAX_SPACE_NAME_SCALARS = 128;
const MAX_SPACE_ICON_SCALARS = 12;
const MAX_RECENT_EMOJIS = 24;

export const LEGACY_SETTINGS_KEYS = [
  LEGACY_PREFERENCE_KEYS.density,
  LEGACY_PREFERENCE_KEYS.sidebarCategory,
  LEGACY_PREFERENCE_KEYS.sidebarSort,
  LEGACY_PREFERENCE_KEYS.collapsedSections,
  LEGACY_PREFERENCE_KEYS.recentEmojis
] as const;
export const LEGACY_NAVIGATION_KEYS = [
  LEGACY_PREFERENCE_KEYS.homeSelection,
  LEGACY_PREFERENCE_KEYS.spacePresentations
] as const;

export type LegacyHomeSelection =
  | { kind: "activity" }
  | { kind: "explore" }
  | { kind: "invites" }
  | { kind: "directMessage"; room_id: string };

export interface LegacySpaceLocalPresentation {
  name?: string;
  icon?: string;
}

export interface LegacyNavigationImport {
  kind: "importLegacy";
  home_selection?: LegacyHomeSelection;
  space_local_presentations: Record<string, LegacySpaceLocalPresentation>;
}

export interface LegacyPreferenceMigration {
  settingsPatch: SettingsPatch;
  navigationImport: LegacyNavigationImport | null;
  sourceKeys: string[];
}

type CurrentLegacySettings = {
  appearance: AppearanceSettings;
  composer: ComposerSettings;
  sidebar: SidebarSettings;
};

export function readLegacyPreferenceMigration(
  storage: Storage,
  validEmojis: ReadonlySet<string>,
  current: CurrentLegacySettings
): LegacyPreferenceMigration {
  const sourceKeys: string[] = [];
  const settingsPatch: SettingsPatch = {};

  const density = storage.getItem(LEGACY_PREFERENCE_KEYS.density);
  if (density === "compact" || density === "default" || density === "comfortable") {
    settingsPatch.appearance = { ...current.appearance, density };
    sourceKeys.push(LEGACY_PREFERENCE_KEYS.density);
  }

  const category = storage.getItem(LEGACY_PREFERENCE_KEYS.sidebarCategory);
  if (category === "dms" || category === "rooms") {
    settingsPatch.sidebar = {
      ...current.sidebar,
      category: category === "dms" ? "people" : "rooms"
    };
    sourceKeys.push(LEGACY_PREFERENCE_KEYS.sidebarCategory);
  }

  const sort = storage.getItem(LEGACY_PREFERENCE_KEYS.sidebarSort);
  if (sort === "active" || sort === "name") {
    settingsPatch.room_list_sort = {
      kind: sort === "active" ? "activity" : "normalLocale"
    };
    sourceKeys.push(LEGACY_PREFERENCE_KEYS.sidebarSort);
  }

  const collapsed = parseRecord(storage.getItem(LEGACY_PREFERENCE_KEYS.collapsedSections));
  if (collapsed) {
    settingsPatch.sidebar = {
      ...(settingsPatch.sidebar ?? current.sidebar),
      collapsed: {
        favourites: collapsed.favourites === true,
        low_priority: collapsed["low-priority"] === true,
        not_joined: collapsed["not-joined"] === true
      }
    };
    sourceKeys.push(LEGACY_PREFERENCE_KEYS.collapsedSections);
  }

  const recent = parseArray(storage.getItem(LEGACY_PREFERENCE_KEYS.recentEmojis));
  if (recent && recent.every((value) => typeof value === "string")) {
    const canonical = distinct(
      recent.filter((value): value is string => validEmojis.has(value))
    ).slice(0, MAX_RECENT_EMOJIS);
    if (canonical.length > 0 || recent.length === 0) {
      settingsPatch.composer = { ...current.composer, recent_emojis: canonical };
      sourceKeys.push(LEGACY_PREFERENCE_KEYS.recentEmojis);
    }
  }

  const homeSelection = parseHomeSelection(
    storage.getItem(LEGACY_PREFERENCE_KEYS.homeSelection)
  );
  if (homeSelection) sourceKeys.push(LEGACY_PREFERENCE_KEYS.homeSelection);

  const spacePresentations = parseSpacePresentations(
    storage.getItem(LEGACY_PREFERENCE_KEYS.spacePresentations)
  );
  if (spacePresentations) sourceKeys.push(LEGACY_PREFERENCE_KEYS.spacePresentations);

  const navigationImport =
    homeSelection || spacePresentations
      ? {
          kind: "importLegacy" as const,
          ...(homeSelection ? { home_selection: homeSelection } : {}),
          space_local_presentations: spacePresentations ?? {}
        }
      : null;

  return { settingsPatch, navigationImport, sourceKeys };
}

export function readBrowserLegacyPreferenceMigration(
  validEmojis: ReadonlySet<string>,
  current: CurrentLegacySettings
): LegacyPreferenceMigration | null {
  return typeof window === "undefined"
    ? null
    : readLegacyPreferenceMigration(window.localStorage, validEmojis, current);
}

export function removeBrowserLegacyPreferenceKeys(keys: readonly string[]): string[] {
  return typeof window === "undefined"
    ? [...keys]
    : removeConfirmedLegacyPreferenceKeys(window.localStorage, keys);
}

export function legacySettingsPatchMatches(
  values: SettingsValues,
  patch: SettingsPatch
): boolean {
  return Object.entries(patch).every(([key, expected]) =>
    JSON.stringify(values[key as keyof SettingsValues]) === JSON.stringify(expected)
  );
}

export function legacyNavigationImportMatches(
  navigation: NavigationState,
  imported: LegacyNavigationImport
): boolean {
  return (
    (imported.home_selection === undefined ||
      JSON.stringify(navigation.home_selection) === JSON.stringify(imported.home_selection)) &&
    Object.keys(navigation.space_local_presentations).length ===
      Object.keys(imported.space_local_presentations).length &&
    Object.entries(imported.space_local_presentations).every(
      ([spaceId, presentation]) =>
        JSON.stringify(navigation.space_local_presentations[spaceId]) ===
        JSON.stringify(presentation)
    )
  );
}

export function keysPresentInMigration(
  migration: LegacyPreferenceMigration,
  allowed: readonly string[]
): string[] {
  const present = new Set(migration.sourceKeys);
  return allowed.filter((key) => present.has(key));
}

export function removeConfirmedLegacyPreferenceKeys(
  storage: Storage,
  keys: readonly string[]
): string[] {
  const failed: string[] = [];
  for (const key of keys) {
    try {
      storage.removeItem(key);
    } catch {
      failed.push(key);
    }
  }
  return failed;
}

function parseHomeSelection(raw: string | null): LegacyHomeSelection | null {
  const value = parseRecord(raw);
  if (!value || typeof value.kind !== "string") return null;
  if (value.kind === "activity" || value.kind === "explore" || value.kind === "invites") {
    return { kind: value.kind };
  }
  if (
    value.kind === "dm" &&
    typeof value.roomId === "string" &&
    matrixRoomIdLike(value.roomId)
  ) {
    return { kind: "directMessage", room_id: value.roomId };
  }
  return null;
}

function parseSpacePresentations(
  raw: string | null
): Record<string, LegacySpaceLocalPresentation> | null {
  const value = parseRecord(raw);
  if (!value) return null;
  const entries: Array<[string, LegacySpaceLocalPresentation]> = [];
  for (const [spaceId, candidate] of Object.entries(value)) {
    if (entries.length === MAX_SPACE_PRESENTATIONS) break;
    if (!matrixRoomIdLike(spaceId) || !isRecord(candidate)) continue;
    const name = boundedTrimmed(candidate.name, MAX_SPACE_NAME_SCALARS);
    const icon = boundedTrimmed(candidate.icon, MAX_SPACE_ICON_SCALARS);
    if (!name && !icon) continue;
    entries.push([spaceId, { ...(name ? { name } : {}), ...(icon ? { icon } : {}) }]);
  }
  return Object.fromEntries(entries);
}

function matrixRoomIdLike(value: string): boolean {
  return value.startsWith("!") && scalarLength(value) <= MAX_SPACE_ID_SCALARS;
}

function boundedTrimmed(value: unknown, maxScalars: number): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed && scalarLength(trimmed) <= maxScalars ? trimmed : null;
}

function scalarLength(value: string): number {
  return [...value].length;
}

function parseRecord(raw: string | null): Record<string, unknown> | null {
  if (raw === null) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return isRecord(value) ? value : null;
  } catch {
    return null;
  }
}

function parseArray(raw: string | null): unknown[] | null {
  if (raw === null) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function distinct(values: readonly string[]): string[] {
  return [...new Set(values)];
}
