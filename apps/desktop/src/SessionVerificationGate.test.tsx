// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { SessionVerificationGate } from "./App";
import { createBrowserFakeApi } from "./backend/browserFakeApi";
import type { DesktopSnapshot, ProvisionalPhase } from "./domain/types";

const provisionalPhaseCases: Array<[ProvisionalPhase, string]> = [
  ["checkingTrust", "Checking device trust…"],
  [{ kind: "checkingTrust" }, "Checking device trust…"],
  ["discoveringMethods", "Discovering verification methods…"],
  [{ kind: "discoveringMethods" }, "Discovering verification methods…"],
  [{ recheckingTrust: { failureKind: "timeout" } }, "Finishing sign-in…"],
  [{ kind: "recheckingTrust", failureKind: "timeout" }, "Finishing sign-in…"],
];

describe("SessionVerificationGate interactions", () => {
  /**
   * #370 disables device-to-device (SAS) verification in the end-user UI. The
   * SDK/core implementation stays intact behind this flag, so the tests that
   * exercise it opt in explicitly. The production default is covered by its own
   * tests below.
   */
  function enableDeviceVerificationForTest(): void {
    vi.stubEnv("VITE_KOUSHI_ENABLE_DEVICE_VERIFICATION", "1");
  }

  function setCleanupSurfaceSession(snapshot: DesktopSnapshot): void {
    snapshot.state.domain.session = {
      kind: "awaitingVerification",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      gate: {
        methods: ["recoveryKey"],
        account_kind: "existingIdentity",
        failureKind: "sdk"
      }
    };
  }

  afterEach(() => {
    cleanup();
    vi.unstubAllEnvs();
  });

  test("production default hides every device-to-device verification entry point", async () => {
    // #370: no SAS/QR action may be visible or keyboard-reachable, and the gate
    // must not be able to start a flow it may not settle. Rust still reports
    // `existingDeviceSas` as available — that projection stays honest; only the
    // UI refuses it.
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "awaitingVerification",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      gate: { methods: ["existingDeviceSas", "recoveryKey"], account_kind: "existingIdentity" }
    };
    const startOwnUserSas = vi.fn(async () => snapshot);
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{ startOwnUserSas, submitRecovery: async () => snapshot }}
      />
    );

    expect(screen.queryByRole("button", { name: "Verify with another device" })).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Try device verification?" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Try device verification anyway" })
    ).toBeNull();
    expect(startOwnUserSas).not.toHaveBeenCalled();

    // Recovery key remains the one enabled verification path.
    expect(screen.getByRole("button", { name: "Verify with recovery key" })).toBeTruthy();
  });

  test("production default does not render SAS emoji comparison while verifying", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "verifying",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      method: "existingDeviceSas",
      flow_id: 370,
      sas_emojis: Array.from({ length: 7 }, (_, index) => ({
        symbol: "🐶",
        description: `emoji-${index}`
      }))
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot
        }}
      />
    );

    expect(screen.queryByRole("button", { name: "They match" })).toBeNull();
    expect(screen.queryByRole("button", { name: "They do not match" })).toBeNull();
    expect(document.querySelector(".session-verification-emojis")).toBeNull();
  });

  test("explains the dead end when no recovery material is available", async () => {
    // Without SAS there is no action left for a user with no recovery key, so
    // the gate must say so rather than render an empty panel. Nothing is
    // deleted here: the session stays signed in (#370).
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "awaitingVerification",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" }
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot
        }}
      />
    );

    expect(
      screen.getByRole("heading", { name: "No recovery key available" })
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Verify with another device" })).toBeNull();
    expect(screen.queryByLabelText("Recovery secret")).toBeNull();
  });

  test.each(provisionalPhaseCases)("renders provisional phase %j without a stale retry action", async (phase, copy) => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "provisional",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      phase,
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{ startOwnUserSas: async () => snapshot, submitRecovery: async () => snapshot }}
      />
    );

    expect(screen.getByText(copy)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test("uses checking-trust copy for both the landmark and heading", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "provisional",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      phase: "checkingTrust",
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
      />
    );

    expect(screen.getByRole("main", { name: "Checking device trust…" })).toBeTruthy();
    expect(
      screen.getByRole("heading", { level: 1, name: "Checking device trust…" })
    ).toBeTruthy();
    expect(screen.queryByText("Verify this session")).toBeNull();
  });

  test("admits SAS and recovery independently and blocks duplicate promise construction", async () => {
    enableDeviceVerificationForTest();
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas", "recoveryKey"], account_kind: "existingIdentity" } };
    let releaseSas!: (value: typeof snapshot) => void;
    const sasPromise = new Promise<typeof snapshot>((resolve) => { releaseSas = resolve; });
    const startOwnUserSas = vi.fn(() => sasPromise);
    const submitRecovery = vi.fn(async () => snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery }} />);

    const sas = screen.getByRole("button", { name: "Verify with another device" });
    const recovery = screen.getByRole("button", { name: "Verify with recovery key" });
    expect(
      recovery.compareDocumentPosition(sas) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    fireEvent.click(sas);
    expect(startOwnUserSas).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Try device verification?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Use recovery key" }));
    expect(screen.queryByRole("dialog", { name: "Try device verification?" })).toBeNull();
    expect(startOwnUserSas).not.toHaveBeenCalled();
    fireEvent.click(sas);
    fireEvent.click(screen.getByRole("button", { name: "Try device verification anyway" }));
    expect(startOwnUserSas).toHaveBeenCalledTimes(1);

    fireEvent.change(screen.getByLabelText("Recovery secret"), { target: { value: "fixture-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify with recovery key" }));
    expect(submitRecovery).toHaveBeenCalledTimes(1);
    releaseSas(snapshot);
  });

  test("rejected operation settles and permits a later attempt", async () => {
    enableDeviceVerificationForTest();
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" } };
    const startOwnUserSas = vi.fn().mockRejectedValueOnce(new Error("rejected")).mockResolvedValue(snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery: async () => snapshot }} />);
    const button = screen.getByRole("button", { name: "Verify with another device" });
    fireEvent.click(button);
    fireEvent.click(screen.getByRole("button", { name: "Try device verification anyway" }));
    await vi.waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByRole("alert").textContent).toContain("Verification command failed");
    fireEvent.click(button);
    fireEvent.click(screen.getByRole("button", { name: "Try device verification anyway" }));
    await vi.waitFor(() => expect(startOwnUserSas).toHaveBeenCalledTimes(2));
  });

  test("does not offer recovery-key fallback when only SAS is available", async () => {
    enableDeviceVerificationForTest();
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" } };
    const startOwnUserSas = vi.fn(async () => snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery: async () => snapshot }} />);

    fireEvent.click(screen.getByRole("button", { name: "Verify with another device" }));

    expect(screen.getByRole("dialog", { name: "Try device verification?" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Use recovery key" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Try device verification anyway" }));
    expect(startOwnUserSas).toHaveBeenCalledTimes(1);
  });

  test("requires consequence confirmation before starting remote-first device cleanup", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "awaitingVerification",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "POISONED",
      gate: {
        methods: ["recoveryKey"],
        account_kind: "existingIdentity",
        failureKind: "sdk",
      },
    };
    snapshot.state.domain.device_cleanup = {
      kind: "offered",
      reason: "recoveryFailed"
    };
    const resolvingSnapshot = structuredClone(snapshot);
    resolvingSnapshot.state.domain.device_cleanup = {
      kind: "resolvingRemote",
      request_id: 370
    };
    const startDeviceCleanup = vi.fn(async () => resolvingSnapshot);
    const onSnapshot = vi.fn();

    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={onSnapshot}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot,
          startDeviceCleanup
        }}
      />
    );

    expect(startDeviceCleanup).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Cancel sign-in and remove this device…",
      })
    );
    const dialog = screen.getByRole("dialog", {
      name: "Cancel sign-in and remove this device",
    });
    expect(dialog).toBeTruthy();
    expect(within(dialog).getByText(/remove this device from your Matrix account first/i)).toBeTruthy();
    expect(within(dialog).getByText(/local messages.*encryption keys/i)).toBeTruthy();
    expect(within(dialog).getByText(/messages on your homeserver are preserved/i)).toBeTruthy();
    expect(within(dialog).getByText(/next sign-in creates a new Device ID/i)).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Remove device and erase local data",
      })
    );

    await vi.waitFor(() => expect(startDeviceCleanup).toHaveBeenCalledTimes(1));
    expect(onSnapshot).toHaveBeenCalledWith(resolvingSnapshot);
  });

  test("submits legacy UIA password through the IME-safe cleanup form", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    setCleanupSurfaceSession(snapshot);
    snapshot.state.domain.device_cleanup = {
      kind: "awaitingUia",
      request_id: 371,
      flow_id: 41
    };
    const submitDeviceCleanupUia = vi.fn(async () => snapshot);
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot,
          submitDeviceCleanupUia
        }}
      />
    );

    const password = screen.getByLabelText("Account password") as HTMLInputElement;
    fireEvent.change(password, { target: { value: "synthetic-password" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue device removal" }));

    await vi.waitFor(() =>
      expect(submitDeviceCleanupUia).toHaveBeenCalledWith(41, "synthetic-password")
    );
    expect(password.value).toBe("");
  });

  test("offers retry and separately confirms local erasure after remote cleanup fails", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    setCleanupSurfaceSession(snapshot);
    snapshot.state.domain.device_cleanup = {
      kind: "remoteFailed",
      request_id: 372,
      auth_mode: "legacy",
      failureKind: "network"
    };
    const startDeviceCleanup = vi.fn(async () => snapshot);
    const eraseLocalDataAnyway = vi.fn(async () => snapshot);
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot,
          startDeviceCleanup,
          eraseLocalDataAnyway
        }}
      />
    );

    expect(
      screen.getByText(/Your credentials and local data are still preserved/)
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry removing device" }));
    await vi.waitFor(() => expect(startDeviceCleanup).toHaveBeenCalledTimes(1));

    const eraseAnywayOffer = screen.getByRole("button", {
      name: "Erase local data anyway…"
    }) as HTMLButtonElement;
    await vi.waitFor(() => expect(eraseAnywayOffer.disabled).toBe(false));
    fireEvent.click(eraseAnywayOffer);
    const dialog = screen.getByRole("dialog", { name: "Erase local data anyway" });
    expect(within(dialog).getByText(/device may remain active on your Matrix account/i)).toBeTruthy();
    expect(eraseLocalDataAnyway).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Erase local data anyway" }));
    await vi.waitFor(() => expect(eraseLocalDataAnyway).toHaveBeenCalledTimes(1));
  });

  test("never asks for a password on the OAuth cleanup failure path", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    setCleanupSurfaceSession(snapshot);
    snapshot.state.domain.device_cleanup = {
      kind: "remoteFailed",
      request_id: 373,
      auth_mode: "oAuth",
      failureKind: "forbidden"
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
      />
    );

    expect(screen.queryByLabelText("Account password")).toBeNull();
    expect(screen.getByRole("button", { name: "Retry removing device" })).toBeTruthy();
  });

  test("shows progress without duplicate cleanup actions while remote removal is pending", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    setCleanupSurfaceSession(snapshot);
    snapshot.state.domain.device_cleanup = {
      kind: "removingRemote",
      request_id: 374,
      auth_mode: "legacy"
    };
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
      />
    );

    expect(screen.getByText("Removing this device from your Matrix account…")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry removing device" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Erase local data anyway…" })).toBeNull();
  });

  test("does not offer destructive cleanup while a recovery retry is verifying", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "verifying",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      gate: {
        methods: ["recoveryKey"],
        account_kind: "existingIdentity",
        failureKind: "sdk"
      },
      method: "recoveryKey",
      flow_id: 375,
      sas_emojis: []
    };
    snapshot.state.domain.device_cleanup = {
      kind: "offered",
      reason: "recoveryFailed"
    };

    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
      />
    );

    expect(
      screen.queryByRole("button", {
        name: "Cancel sign-in and remove this device…"
      })
    ).toBeNull();
  });

  test("provides a primary-button-only verification window drag region", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "awaitingVerification",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" },
    };
    const onStartWindowDrag = vi.fn();
    const { container } = render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        onStartWindowDrag={onStartWindowDrag}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot,
        }}
      />
    );

    const dragRegion = container.querySelector(".session-verification-drag-region");
    expect(dragRegion?.getAttribute("data-tauri-drag-region")).toBe("");
    fireEvent.mouseDown(dragRegion!, { button: 2, buttons: 2 });
    expect(onStartWindowDrag).not.toHaveBeenCalled();
    fireEvent.mouseDown(dragRegion!, { button: 0, buttons: 1 });
    expect(onStartWindowDrag).toHaveBeenCalledTimes(1);
  });
});
