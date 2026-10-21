import { useSyncExternalStore } from "react";
import { clearIntlFormatCache } from "../../domain/intlFormatCache";

const listeners = new Set<() => void>();
let revision = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

function scheduleMidnight() {
  clearTimeout(timer);
  const now = new Date();
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  timer = setTimeout(refresh, midnight.getTime() - now.getTime());
}

function refresh() {
  // Also refresh cached time-zone patterns when a suspended window resumes.
  clearIntlFormatCache();
  revision += 1;
  for (const listener of listeners) listener();
  if (listeners.size > 0) scheduleMidnight();
}

function onVisibilityChange() {
  if (document.visibilityState === "visible") refresh();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    scheduleMidnight();
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", onVisibilityChange);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      clearTimeout(timer);
      timer = undefined;
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    }
  };
}

/** One cleanup-safe presentation clock shared by every mounted timestamp. */
export function useCalendarPresentation(): void {
  useSyncExternalStore(subscribe, () => revision, () => 0);
}
