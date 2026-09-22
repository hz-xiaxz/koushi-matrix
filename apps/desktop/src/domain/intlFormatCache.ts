/**
 * Shared `Intl` formatters, one per locale and option set.
 *
 * Constructing an `Intl` formatter resolves the locale and loads its pattern
 * data every time, which costs far more than formatting with it. Timeline rows
 * format a timestamp on every render, so a formatter built per call shows up
 * as locale parsing in a renderer profile (#969). The key space is the handful
 * of option literals in the source times the UI locales, so it stays small.
 *
 * A formatter resolves the system time zone and default locale when it is
 * built, so the cache is dropped whenever the window regains focus: a laptop
 * that changed time zone while the app sat in the tray formats correctly again
 * without a restart.
 */
const dateTimeFormats = new Map<string, Intl.DateTimeFormat>();
const listFormats = new Map<string, Intl.ListFormat>();

function cacheKey(locale: string | undefined, options: object): string {
  return `${locale ?? ""}|${JSON.stringify(options)}`;
}

export function cachedDateTimeFormat(
  locale: string | undefined,
  options: Intl.DateTimeFormatOptions
): Intl.DateTimeFormat {
  const key = cacheKey(locale, options);
  let format = dateTimeFormats.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, options);
    dateTimeFormats.set(key, format);
  }
  return format;
}

export function cachedListFormat(
  locale: string | undefined,
  options: Intl.ListFormatOptions
): Intl.ListFormat {
  const key = cacheKey(locale, options);
  let format = listFormats.get(key);
  if (!format) {
    format = new Intl.ListFormat(locale, options);
    listFormats.set(key, format);
  }
  return format;
}

export function clearIntlFormatCache(): void {
  dateTimeFormats.clear();
  listFormats.clear();
}

if (typeof window !== "undefined") {
  window.addEventListener("focus", clearIntlFormatCache);
}
