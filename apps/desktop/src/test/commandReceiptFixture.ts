import type { DesktopApi } from "../backend/desktopApi";
import type {
  CommandAdmission,
  CommandReceipt,
  CommandSettlement,
  DesktopSnapshot,
  SubmissionResponse
} from "../domain/types";

export async function commandSnapshot(
  api: DesktopApi,
  command: Promise<CommandReceipt>
): Promise<DesktopSnapshot> {
  await command;
  return api.settlementSnapshot();
}

export async function submissionResponseSnapshot(
  api: DesktopApi,
  command: Promise<SubmissionResponse>
): Promise<SubmissionResponse & { snapshot: DesktopSnapshot }> {
  const response = await command;
  return { ...response, snapshot: await api.settlementSnapshot() };
}

export function installCommandSnapshotQueue(api: DesktopApi) {
  const original = api.settlementSnapshot.bind(api);
  const queued: DesktopSnapshot[] = [];
  api.settlementSnapshot = async () => {
    const actual = await original();
    if (queued.length === 0) return actual;
    const latest = queued.reduce((left, right) =>
      (right.state_generation ?? 0) >= (left.state_generation ?? 0) ? right : left
    );
    queued.length = 0;
    return structuredClone(
      (latest.state_generation ?? 0) >= (actual.state_generation ?? 0) ? latest : actual
    );
  };
  const generation = (snapshot: DesktopSnapshot) => snapshot.state_generation ?? 0;
  return {
    settlement(command: Promise<DesktopSnapshot>): Promise<CommandSettlement> {
      return command.then((snapshot) => {
        queued.push(snapshot);
        return { protocolVersion: 1, publishedGeneration: generation(snapshot) };
      });
    },
    admission(command: Promise<DesktopSnapshot>): Promise<CommandAdmission> {
      return command.then((snapshot) => {
        queued.push(snapshot);
        return { protocolVersion: 1, admittedGeneration: generation(snapshot) };
      });
    }
  };
}
