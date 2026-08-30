import type { CoreEventPayload, StateUpdateEnvelope } from "../domain/coreEvents";

export type DesktopEventUnlisten = () => void;

export interface DesktopEventPort {
  listenCoreEvents(
    listener: (payload: CoreEventPayload) => void
  ): Promise<DesktopEventUnlisten>;
  listenMenuActions(listener: (payload: string) => void): Promise<DesktopEventUnlisten>;
  listenStateUpdates(
    listener: (payload: StateUpdateEnvelope) => void
  ): Promise<DesktopEventUnlisten>;
}
