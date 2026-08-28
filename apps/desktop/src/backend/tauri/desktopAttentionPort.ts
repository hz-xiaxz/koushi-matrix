import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  cancelAll,
  isPermissionGranted,
  removeAllActive,
  sendNotification
} from "@tauri-apps/plugin-notification";

import type { DesktopAttentionPort } from "../desktopAttentionPort";

export function createTauriDesktopAttentionPort(): DesktopAttentionPort {
  let permissionPromise: Promise<boolean> | null = null;

  return {
    currentWindow: getCurrentWindow,
    notifications: {
      async notify(content) {
        permissionPromise ??= isPermissionGranted();
        if (!(await permissionPromise)) {
          return;
        }
        await sendNotification(content);
      },
      async clear() {
        const outcomes = await Promise.allSettled([cancelAll(), removeAllActive()]);
        if (outcomes.some((outcome) => outcome.status === "rejected")) {
          throw new Error("native_notification_clear_failed");
        }
      }
    },
    sound: {
      playAttentionSound: () =>
        invoke<"played" | "unsupported" | "failed" | "skipped">(
          "play_native_attention_sound"
        )
    },
    nativeBadge: {
      setBadgeCount: (count?: number) =>
        invoke<"applied" | "unsupported" | "mismatch">("set_native_attention_badge", {
          count
        })
    }
  };
}
