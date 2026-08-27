import { getCurrentWindow } from "@tauri-apps/api/window";

import { createBrowserFakeApi } from "./browserFakeApi";
import { TauriDesktopApi } from "./client";
import type { DesktopApi } from "./desktopApi";
import { isTauriRuntime } from "./tauriTimelineTransport";

export const api: DesktopApi = isTauriRuntime()
  ? new TauriDesktopApi()
  : createBrowserFakeApi();

export function startSessionVerificationWindowDrag(): void {
  if (!isTauriRuntime()) return;
  void getCurrentWindow().startDragging().catch(() => undefined);
}
