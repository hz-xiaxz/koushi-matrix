import { createBrowserFakeApi } from "./browserFakeApi";
import { TauriDesktopApi } from "./client";
import type { DesktopApi } from "./desktopApi";
import { isTauriRuntime } from "./runtimeEnvironment";
import { windowDialogPort } from "./windowDialogRuntime";

export const api: DesktopApi = isTauriRuntime()
  ? new TauriDesktopApi()
  : createBrowserFakeApi();

export function startSessionVerificationWindowDrag(): void {
  if (!isTauriRuntime()) return;
  void windowDialogPort.startDragging().catch(() => undefined);
}
