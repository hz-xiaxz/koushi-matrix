import type {
  ComposerKeyEvent,
  ComposerResolvedAction,
  ComposerResolverOptions,
  DesktopSnapshot,
  LocaleDisplayProfile,
  LocaleSettings,
  SettingsPatch
} from "../../domain/types";

export function defaultSettingsState(): DesktopSnapshot["state"]["domain"]["settings"] {
  return {
    values: {
      locale: { language_tag: null, text_direction: "auto" },
      appearance: { theme: "system" },
      typography: { font: "system", emoji: "system" },
      keyboard: { composer_send_shortcut: "enter" },
      composer: { math_mode: true },
      notifications: {
        desktop_notifications: true,
        sound: true,
        badges: true,
        send_read_receipts: true,
        send_typing_notifications: true
      },
      display: {
        code_block_wrap: true,
        hide_redacted: true,
        url_previews_enabled: true,
        encrypted_url_previews_enabled: true
      },
      media: {
        image_upload_compression_policy: {
          threshold_bytes: 1048576,
          threshold_long_edge: 2560,
          target_long_edge: 2048,
          quality_percent: 82
        }
      },
      timeline: {
        auto_load_older_messages: true,
        // Mirrors the Rust product default (#366): latest-reply placement on.
        thread_root_order: { kind: "latestReply" }
      },
      search_crawler: {
        speed: "standard" as const,
        include_media_captions: true,
        include_filenames: true
      },
      thread_list_order: { kind: "latestReply" },
      room_list_sort: { kind: "activity" }
    },
    persistence: { kind: "idle" }
  };
}

export function defaultLocaleDisplayProfile(): LocaleDisplayProfile {
  return resolveLocaleDisplayProfile({ language_tag: null, text_direction: "auto" });
}

export function defaultTypographyDisplayProfile(): DesktopSnapshot["state"]["domain"]["typography_profile"] {
  return resolveTypographyDisplayProfile({ font: "system", emoji: "system" });
}

export function resolveTypographyDisplayProfile(
  typography: DesktopSnapshot["state"]["domain"]["settings"]["values"]["typography"]
): DesktopSnapshot["state"]["domain"]["typography_profile"] {
  return {
    font: typography.font,
    emoji: typography.emoji,
    platform: "linux",
    font_asset: typography.font === "inter" ? "bundledPreferred" : "systemFallback",
    emoji_asset: typography.emoji === "twemojiColr" ? "bundledPreferred" : "systemFallback"
  };
}

export function resolveLocaleDisplayProfile(locale: LocaleSettings): LocaleDisplayProfile {
  const parsed = parseLocale(locale.language_tag);
  const pseudoLocale = parsed?.pseudo_locale ?? "none";
  const catalogLocale =
    pseudoLocale === "accented" || pseudoLocale === "bidi"
      ? "pseudo"
      : parsed?.language === "ja"
        ? "ja"
        : "en";
  const lang =
    pseudoLocale === "accented"
      ? "en-XA"
      : pseudoLocale === "bidi"
        ? "ar-XB"
        : catalogLocale === "ja"
          ? "ja"
          : "en";
  const dir =
    locale.text_direction === "ltr" || locale.text_direction === "rtl"
      ? locale.text_direction
      : pseudoLocale === "bidi" || parsed?.direction === "rtl"
        ? "rtl"
        : "ltr";

  return {
    lang,
    dir,
    catalog_locale: catalogLocale,
    pseudo_locale: pseudoLocale,
    platform: "linux",
    modifier_labels: { primary: "Ctrl" }
  };
}

function parseLocale(
  rawTag: string | null
): { language: "en" | "ja" | "rtl"; direction: "ltr" | "rtl"; pseudo_locale: "none" | "accented" | "bidi" } | null {
  const normalized = rawTag?.trim().replaceAll("_", "-");
  if (!normalized) {
    return null;
  }
  const [primaryRaw, ...rest] = normalized.split("-");
  const primary = primaryRaw.toLowerCase();
  if (!/^[a-z]{2,3}$/.test(primary) || rest.some((subtag) => subtag.toLowerCase() === "x")) {
    return null;
  }
  if (!rest.every((subtag) => /^[a-z0-9]{1,8}$/i.test(subtag))) {
    return null;
  }
  const pseudo_locale =
    normalized.toLowerCase() === "en-xa"
      ? "accented"
      : normalized.toLowerCase() === "ar-xb"
        ? "bidi"
        : "none";

  if (primary === "en") {
    return { language: "en", direction: "ltr", pseudo_locale };
  }
  if (primary === "ja") {
    return { language: "ja", direction: "ltr", pseudo_locale };
  }
  if (["ar", "dv", "fa", "he", "ps", "sd", "ug", "ur", "yi"].includes(primary)) {
    return { language: "rtl", direction: "rtl", pseudo_locale };
  }
  return null;
}

export function applySettingsPatch(
  values: DesktopSnapshot["state"]["domain"]["settings"]["values"],
  patch: SettingsPatch
): DesktopSnapshot["state"]["domain"]["settings"]["values"] {
  return {
    locale: patch.locale ?? values.locale,
    appearance: patch.appearance ?? values.appearance,
    typography: patch.typography ?? values.typography,
    keyboard: patch.keyboard ?? values.keyboard,
    composer: patch.composer ?? values.composer ?? { math_mode: true },
    notifications: patch.notifications ?? values.notifications,
    display: patch.display ?? values.display,
    media: patch.media ?? values.media,
    timeline: patch.timeline ?? values.timeline,
    search_crawler: patch.search_crawler ?? values.search_crawler,
    thread_list_order: patch.thread_list_order ?? values.thread_list_order,
    room_list_sort: patch.room_list_sort ?? values.room_list_sort
  };
}

export function resolveComposerKeyActionFromSettings(
  sendShortcut: DesktopSnapshot["state"]["domain"]["settings"]["values"]["keyboard"]["composer_send_shortcut"],
  keyEvent: ComposerKeyEvent,
  options: ComposerResolverOptions
): ComposerResolvedAction {
  if (keyEvent.is_composing) {
    return "commitImeCandidate";
  }
  if (keyEvent.key === "escape") {
    return options.autocomplete_open ? "closeAutocomplete" : "cancel";
  }
  if (keyEvent.key !== "enter") {
    return "noop";
  }
  if (keyEvent.modifiers.shift || keyEvent.modifiers.alt) {
    return "insertNewline";
  }
  if (options.autocomplete_open) {
    return "acceptAutocomplete";
  }
  const wantsSend =
    sendShortcut === "enter" ||
    (sendShortcut === "modEnter" && (keyEvent.modifiers.ctrl || keyEvent.modifiers.meta));
  if (!wantsSend) {
    return "insertNewline";
  }
  return options.send_enabled ? "send" : "noop";
}
