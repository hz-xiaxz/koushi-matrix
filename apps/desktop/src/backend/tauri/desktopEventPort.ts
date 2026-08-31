import { listen } from "@tauri-apps/api/event";

import type { CoreEventPayload, StateUpdateEnvelope } from "../../domain/coreEvents";
import type { DesktopEventPort } from "../desktopEventPort";

const CORE_EVENT_NAME = "koushi-desktop://event";
const MENU_EVENT_NAME = "koushi-desktop://menu";
const STATE_UPDATE_EVENT_NAME = "koushi-desktop://state-update";

export function createTauriDesktopEventPort(): DesktopEventPort {
  return {
    listenCoreEvents(listener) {
      return listen<CoreEventPayload>(CORE_EVENT_NAME, (event) => listener(event.payload));
    },
    listenMenuActions(listener) {
      return listen<string>(MENU_EVENT_NAME, (event) => listener(event.payload));
    },
    listenStateUpdates(listener) {
      return listen<StateUpdateEnvelope>(STATE_UPDATE_EVENT_NAME, (event) =>
        listener(event.payload)
      );
    }
  };
}
