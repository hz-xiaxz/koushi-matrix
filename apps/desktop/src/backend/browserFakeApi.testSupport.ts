import type { DesktopApi } from "./desktopApi";
import type {
  CommandAdmission,
  CommandReceipt,
  CommandSettlement,
  DesktopSnapshot,
  SubmissionResponse
} from "../domain/types";

export async function browserCommandSnapshot(
  api: DesktopApi,
  command: Promise<CommandReceipt>
): Promise<DesktopSnapshot> {
  await command;
  return api.settlementSnapshot();
}

export async function browserSubmissionResponse(
  api: DesktopApi,
  command: Promise<SubmissionResponse>
): Promise<SubmissionResponse & { snapshot: DesktopSnapshot }> {
  const response = await command;
  return { ...response, snapshot: await api.settlementSnapshot() };
}

export function installLegacyCommandSnapshotBridge(api: DesktopApi) {
  const original = api.settlementSnapshot.bind(api);
  const overrides: DesktopSnapshot[] = [];
  api.settlementSnapshot = async () => {
    const actual = await original();
    if (overrides.length === 0) return actual;
    const override = overrides.reduce((latest, candidate) =>
      (candidate.state_generation ?? 0) >= (latest.state_generation ?? 0)
        ? candidate
        : latest
    );
    overrides.length = 0;
    return structuredClone(
      (override.state_generation ?? 0) >= (actual.state_generation ?? 0) ? override : actual
    );
  };
  const generation = (snapshot: DesktopSnapshot) => snapshot.state_generation ?? 0;
  return {
    settlement(command: Promise<DesktopSnapshot>): Promise<CommandSettlement> {
      return command.then((snapshot) => {
        overrides.push(snapshot);
        return { protocolVersion: 1, publishedGeneration: generation(snapshot) };
      });
    },
    admission(command: Promise<DesktopSnapshot>): Promise<CommandAdmission> {
      return command.then((snapshot) => {
        overrides.push(snapshot);
        return { protocolVersion: 1, admittedGeneration: generation(snapshot) };
      });
    }
  };
}
