import type { StateUpdateEnvelope } from "./coreEvents";
import type { DesktopSnapshot } from "./types";

export interface StateUpdateConsumer {
  initialize(snapshot: DesktopSnapshot): void;
  recoverInitial(): void;
  receive(envelope: StateUpdateEnvelope): void;
  dispose(): void;
}

interface StateUpdateConsumerOptions {
  applySnapshot(snapshot: DesktopSnapshot): void;
  applyDelta(envelope: Extract<StateUpdateEnvelope, { kind: "delta" }>): boolean;
  resetTimeline(): void;
  resyncSnapshot(): Promise<DesktopSnapshot>;
}

/**
 * Owns the renderer's one ordered application-state lane. Updates received
 * while the initial snapshot is being admitted are replayed in order; a
 * missed generation is recovered once from the Tauri snapshot/replay command.
 */
export function createStateUpdateConsumer(
  options: StateUpdateConsumerOptions
): StateUpdateConsumer {
  let generation: number | null = null;
  let queued: StateUpdateEnvelope[] = [];
  let resyncInFlight = false;
  let resyncNeeded = false;
  let disposed = false;

  function receive(envelope: StateUpdateEnvelope): void {
    if (disposed) return;
    if (generation === null || resyncInFlight || resyncNeeded) {
      queued.push(envelope);
      if (resyncNeeded && !resyncInFlight) startResync();
      return;
    }
    consume(envelope);
  }

  function initialize(snapshot: DesktopSnapshot): void {
    if (disposed || generation !== null) return;
    const initialGeneration = snapshot.state_generation;
    if (initialGeneration === undefined) return;
    options.applySnapshot(snapshot);
    generation = initialGeneration;
    drain();
  }

  function consume(envelope: StateUpdateEnvelope): void {
    if (envelope.generation <= (generation ?? -1)) return;

    if (envelope.kind === "snapshot") {
      if (envelope.snapshot.state_generation !== envelope.generation) {
        startResync();
        return;
      }
      if (envelope.reason === "gap") options.resetTimeline();
      options.applySnapshot(envelope.snapshot);
      generation = envelope.generation;
      return;
    }

    if (envelope.generation !== generation! + 1) {
      queued.unshift(envelope);
      startResync();
      return;
    }

    if (!options.applyDelta(envelope)) {
      queued.unshift(envelope);
      startResync();
      return;
    }
    generation = envelope.generation;
  }

  function startResync(): void {
    if (resyncInFlight || disposed) return;
    resyncInFlight = true;
    resyncNeeded = false;
    options.resetTimeline();
    void options.resyncSnapshot().then(
      (snapshot) => {
        if (disposed) return;
        const resyncGeneration = snapshot.state_generation;
        if (resyncGeneration === undefined) {
          resyncNeeded = true;
          resyncInFlight = false;
          return;
        }
        options.applySnapshot(snapshot);
        generation = resyncGeneration;
        resyncInFlight = false;
        drain();
      },
      () => {
        if (disposed) return;
        resyncNeeded = true;
        resyncInFlight = false;
      }
    );
  }

  function drain(): void {
    while (!disposed && !resyncInFlight && queued.length > 0) {
      const pending = queued.shift()!;
      consume(pending);
    }
  }

  return {
    initialize,
    recoverInitial() {
      if (disposed || generation !== null) return;
      resyncNeeded = true;
      startResync();
    },
    receive,
    dispose() {
      disposed = true;
      queued = [];
    }
  };
}
