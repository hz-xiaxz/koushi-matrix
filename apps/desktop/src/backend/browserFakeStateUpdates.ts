import type { StateUpdateEnvelope } from "../domain/coreEvents";

const listeners = new Set<(update: StateUpdateEnvelope) => void>();

export function emitBrowserFakeStateUpdate(update: StateUpdateEnvelope): void {
  for (const listener of listeners) listener(update);
}

export async function listenBrowserFakeStateUpdates(
  listener: (update: StateUpdateEnvelope) => void
): Promise<() => void> {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
