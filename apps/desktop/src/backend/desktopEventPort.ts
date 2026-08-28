import type { CoreEventPayload } from "../domain/coreEvents";

export type DesktopEventUnlisten = () => void;

export interface DesktopEventPort {
  listenCoreEvents(
    listener: (payload: CoreEventPayload) => void
  ): Promise<DesktopEventUnlisten>;
  listenMenuActions(listener: (payload: string) => void): Promise<DesktopEventUnlisten>;
  listenStateChanges(listener: () => void): Promise<DesktopEventUnlisten>;
}
