import type { CoreEventPayload, StateUpdateEnvelope } from "../domain/coreEvents";
import type { AccountTabsSnapshot, DesktopUpdateState, McpAssistantsState } from "../domain/types";

export type DesktopEventUnlisten = () => void;

export interface DesktopEventPort {
  listenCoreEvents(
    listener: (payload: CoreEventPayload) => void
  ): Promise<DesktopEventUnlisten>;
  listenMenuActions(listener: (payload: string) => void): Promise<DesktopEventUnlisten>;
  listenStateUpdates(
    listener: (payload: StateUpdateEnvelope) => void
  ): Promise<DesktopEventUnlisten>;
  listenAccountTabs(
    listener: (payload: AccountTabsSnapshot) => void
  ): Promise<DesktopEventUnlisten>;
  listenDesktopUpdates(
    listener: (payload: DesktopUpdateState) => void
  ): Promise<DesktopEventUnlisten>;
  listenMcpAssistants(
    listener: (payload: McpAssistantsState) => void
  ): Promise<DesktopEventUnlisten>;
}
