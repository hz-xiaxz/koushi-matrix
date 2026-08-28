import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  confirm as confirmDialog,
  open as openDialog,
  save as saveDialog
} from "@tauri-apps/plugin-dialog";

import type { WindowDialogPort } from "../windowDialogPort";

export function createTauriWindowDialogPort(): WindowDialogPort {
  return {
    async toggleFullscreen() {
      const window = getCurrentWindow();
      const fullscreen = await window.isFullscreen();
      await window.setFullscreen(!fullscreen);
    },
    startDragging() {
      return getCurrentWindow().startDragging();
    },
    confirm(message, options) {
      return confirmDialog(message, options);
    },
    saveFile(options) {
      return saveDialog(options);
    },
    openFile(options) {
      return openDialog(options);
    }
  };
}
