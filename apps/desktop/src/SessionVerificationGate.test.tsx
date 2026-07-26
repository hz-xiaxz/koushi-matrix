// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { SessionVerificationGate } from "./App";
import { createBrowserFakeApi } from "./backend/browserFakeApi";
import type { ProvisionalPhase } from "./domain/types";

const provisionalPhaseCases: Array<[ProvisionalPhase, string, boolean]> = [
  ["checkingTrust", "Checking device trust…", false],
  ["discoveringMethods", "Discovering verification methods…", true],
  [{ recheckingTrust: { failureKind: "timeout" } }, "Finishing sign-in…", true],
];

describe("SessionVerificationGate interactions", () => {
  afterEach(cleanup);

  test.each(provisionalPhaseCases)("renders provisional phase %j with phase-specific retry availability", async (phase, copy, retryVisible) => {
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
        operations={{ startOwnUserSas: async () => snapshot, submitRecovery: async () => snapshot, retryCurrentDeviceTrustDiscovery: async () => snapshot }}
      />
    );

    expect(screen.getByText(copy)).toBeTruthy();
    const retry = screen.queryByRole("button", { name: "Retry" });
    expect(Boolean(retry)).toBe(retryVisible);
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

  test("blocks duplicate retry promise construction while discovery is pending", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = {
      kind: "provisional",
      user_id: "@u:example.invalid",
      homeserver: "https://example.invalid",
      device_id: "D",
      phase: "discoveringMethods",
    };
    let releaseRetry!: (value: typeof snapshot) => void;
    const retryPromise = new Promise<typeof snapshot>((resolve) => { releaseRetry = resolve; });
    const retryCurrentDeviceTrustDiscovery = vi.fn(() => retryPromise);
    render(
      <SessionVerificationGate
        snapshot={snapshot}
        onSnapshot={() => undefined}
        onSignOut={() => undefined}
        operations={{
          startOwnUserSas: async () => snapshot,
          submitRecovery: async () => snapshot,
          retryCurrentDeviceTrustDiscovery,
        }}
      />
    );

    const retry = screen.getByRole("button", { name: "Retry" });
    fireEvent.click(retry);
    fireEvent.click(retry);
    expect(retryCurrentDeviceTrustDiscovery).toHaveBeenCalledTimes(1);
    releaseRetry(snapshot);
  });

  test("admits SAS and recovery independently and blocks duplicate promise construction", async () => {
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas", "recoveryKey"], account_kind: "existingIdentity" } };
    let releaseSas!: (value: typeof snapshot) => void;
    const sasPromise = new Promise<typeof snapshot>((resolve) => { releaseSas = resolve; });
    const startOwnUserSas = vi.fn(() => sasPromise);
    const submitRecovery = vi.fn(async () => snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery, retryCurrentDeviceTrustDiscovery: async () => snapshot }} />);

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
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" } };
    const startOwnUserSas = vi.fn().mockRejectedValueOnce(new Error("rejected")).mockResolvedValue(snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery: async () => snapshot, retryCurrentDeviceTrustDiscovery: async () => snapshot }} />);
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
    const snapshot = await createBrowserFakeApi({ session: "needsRecovery" }).getSnapshot();
    snapshot.state.domain.session = { kind: "awaitingVerification", user_id: "@u:example.invalid", homeserver: "https://example.invalid", device_id: "D", gate: { methods: ["existingDeviceSas"], account_kind: "existingIdentity" } };
    const startOwnUserSas = vi.fn(async () => snapshot);
    render(<SessionVerificationGate snapshot={snapshot} onSnapshot={() => undefined} onSignOut={() => undefined} operations={{ startOwnUserSas, submitRecovery: async () => snapshot, retryCurrentDeviceTrustDiscovery: async () => snapshot }} />);

    fireEvent.click(screen.getByRole("button", { name: "Verify with another device" }));

    expect(screen.getByRole("dialog", { name: "Try device verification?" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Use recovery key" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Try device verification anyway" }));
    expect(startOwnUserSas).toHaveBeenCalledTimes(1);
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
          retryCurrentDeviceTrustDiscovery: async () => snapshot,
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
