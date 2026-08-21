import { createDesktopApi } from "./client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "./tauriTimelineTransport";

export const api = createDesktopApi();

export function startSessionVerificationWindowDrag(): void {
  if (!isTauriRuntime()) return;
  void getCurrentWindow().startDragging().catch(() => undefined);
}
