import { describe, expect, test } from "vitest";

import generatedCoreEvents from "./coreEvents.generated.json";
import type { RoomListReadiness, RoomListSource, SessionState } from "./types";

const blockedSession = (
  failure: "unsupported" | "unreachable" | "invalidResponse"
): SessionState => ({
  kind: "capabilityBlocked",
  homeserver: "https://matrix.example.test",
  user_id: "@qa:example.test",
  device_id: "QADEVICE",
  failure
});

describe("sync wire contract", () => {
  test("projects all capability outcomes and readiness states without legacy tokens", () => {
    const capabilityOutcomes: SessionState[] = [
      {
        kind: "ready",
        homeserver: "https://matrix.example.test",
        user_id: "@qa:example.test",
        device_id: "QADEVICE"
      },
      blockedSession("unsupported"),
      blockedSession("unreachable"),
      blockedSession("invalidResponse")
    ];
    const readinessStates: RoomListReadiness[] = [
      { kind: "uninitialized" },
      { kind: "loading", source: "cache", generation: 1 },
      { kind: "ready", source: "live", generation: 2 },
      { kind: "failed", source: "live", generation: 3, failureKind: "service" }
    ];

    expect(capabilityOutcomes.map((state) => state.kind)).toEqual([
      "ready",
      "capabilityBlocked",
      "capabilityBlocked",
      "capabilityBlocked"
    ]);
    expect(readinessStates).toHaveLength(4);
    expect(generatedCoreEvents).toHaveProperty("syncStarted");
    expect(JSON.stringify(generatedCoreEvents)).not.toMatch(
      /SyncMode|syncModeChanged|syncService|legacy|LegacySync/
    );
  });

  test("rejects obsolete source and incomplete capability-blocked states at compile time", () => {
    const live: RoomListSource = "live";
    expect(live).toBe("live");

    // @ts-expect-error Legacy room-list source is no longer a wire state.
    const legacy: RoomListSource = "legacy";
    expect(legacy).toBe("legacy");

    // @ts-expect-error Capability-blocked state must carry a typed failure.
    const incomplete: SessionState = {
      kind: "capabilityBlocked",
      homeserver: "https://matrix.example.test",
      user_id: "@qa:example.test",
      device_id: "QADEVICE"
    };
    expect(incomplete).toBeDefined();

    // @ts-expect-error Ready must carry the complete account identity.
    const incompleteReady: SessionState = { kind: "ready" };
    expect(incompleteReady).toBeDefined();

    // @ts-expect-error Session-only failure tokens cannot appear on signed-out state.
    const invalidSignedOut: SessionState = { kind: "signedOut", failure: "unsupported" };
    expect(invalidSignedOut).toBeDefined();
  });
});
