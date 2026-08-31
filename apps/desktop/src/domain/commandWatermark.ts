import type { CommandReceipt, DesktopSnapshot } from "./types";

export function commandReceiptGeneration(receipt: CommandReceipt): number {
  return "publishedGeneration" in receipt
    ? receipt.publishedGeneration
    : receipt.admittedGeneration;
}

interface CommandWatermarkOptions {
  currentGeneration: () => number | null;
  settlementSnapshot: () => Promise<DesktopSnapshot>;
  applySnapshot: (snapshot: DesktopSnapshot) => void;
}

export function createCommandReceiptReconciler(options: CommandWatermarkOptions) {
  let requiredGeneration = -1;
  let inFlight: Promise<void> | null = null;

  return async (receipt: CommandReceipt): Promise<void> => {
    const generation = commandReceiptGeneration(receipt);
    requiredGeneration = Math.max(requiredGeneration, generation);
    while ((options.currentGeneration() ?? -1) < generation) {
      if (!inFlight) {
        const requestedGeneration = requiredGeneration;
        inFlight = (async () => {
          const snapshot = await options.settlementSnapshot();
          const snapshotGeneration = snapshot.state_generation;
          if (snapshotGeneration === undefined) {
            throw new Error("settlement snapshot has no generation");
          }
          const liveGeneration = options.currentGeneration() ?? -1;
          if (
            snapshotGeneration < requestedGeneration &&
            liveGeneration < requestedGeneration
          ) {
            throw new Error("settlement snapshot is older than the command receipt");
          }
          if (snapshotGeneration > liveGeneration) {
            options.applySnapshot(snapshot);
          }
        })().finally(() => {
          inFlight = null;
        });
      }
      await inFlight;
    }
  };
}
