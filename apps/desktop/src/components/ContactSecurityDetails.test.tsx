// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import { ProfilePanel } from "./PeoplePanel";
import type {
  ContactSecurityState,
  ContactSecuritySummary,
  RoomManagementState
} from "../domain/types";

const CONTACT = "@ada:example.invalid";
const OTHER = "@grace:example.invalid";
const ME = "@current:example.invalid";

const roomManagement: RoomManagementState = {
  selected_room_id: null,
  settings: null,
  operation: { kind: "idle" }
};

function summary(partial: Partial<ContactSecuritySummary>): ContactSecuritySummary {
  return {
    devices: "allOwnerSigned",
    device_counts: {
      total: 2,
      owner_signed: 2,
      not_owner_signed: 0,
      owner_signature_invalid: 0,
      excluded_dehydrated: 0
    },
    device_signatures: ["ownerSigned", "ownerSigned"],
    identity: "notVerifiedByYou",
    ...partial
  };
}

const unsignedDeviceVerified = summary({
  devices: "someNotOwnerSigned",
  device_counts: {
    total: 3,
    owner_signed: 1,
    not_owner_signed: 2,
    owner_signature_invalid: 1,
    excluded_dehydrated: 1
  },
  device_signatures: ["ownerSigned", "notOwnerSigned", "ownerSignatureInvalid"],
  identity: "verifiedByYou"
});

function loaded(value: ContactSecuritySummary, userId = CONTACT): ContactSecurityState {
  return { user_id: userId, load: { kind: "loaded", request_id: 4 }, summary: value };
}

function renderProfile(state: ContactSecurityState, userId = CONTACT) {
  const actions = { load: vi.fn(), close: vi.fn() };
  const view = render(
    <ProfilePanel
      userId={userId}
      currentUserId={ME}
      roomOrSpace={null}
      roomManagement={roomManagement}
      profileUsers={{}}
      contactSecurity={state}
      contactSecurityActions={actions}
      onBack={() => undefined}
    />
  );
  return { actions, ...view };
}

function row(label: string): HTMLElement {
  const element = screen.getByText(label).closest(".profile-security-row");
  if (!(element instanceof HTMLElement)) throw new Error(`missing row ${label}`);
  return element;
}

afterEach(cleanup);

describe("ContactSecurityDetails", () => {
  test("opening User info dispatches only the read-only load; closing dispatches close", () => {
    const { actions, rerender, unmount } = renderProfile({
      user_id: null,
      load: { kind: "idle" },
      summary: null
    });
    expect(actions.load).toHaveBeenCalledTimes(1);
    expect(actions.load).toHaveBeenCalledWith(CONTACT);
    expect(actions.close).not.toHaveBeenCalled();
    expect(within(row("Their devices")).getByText("Checking…")).toBeTruthy();

    // Switching contact closes the previous one and loads the new one.
    rerender(
      <ProfilePanel
        userId={OTHER}
        currentUserId={ME}
        roomOrSpace={null}
        roomManagement={roomManagement}
        profileUsers={{}}
        contactSecurity={loaded(unsignedDeviceVerified)}
        contactSecurityActions={actions}
        onBack={() => undefined}
      />
    );
    expect(actions.close).toHaveBeenCalledTimes(1);
    expect(actions.load).toHaveBeenLastCalledWith(OTHER);
    // The snapshot still belongs to the previous contact: not shown as Grace's.
    expect(within(row("Their devices")).getByText("Checking…")).toBeTruthy();
    expect(screen.queryByText("Verified by you")).toBeNull();

    unmount();
    expect(actions.close).toHaveBeenCalledTimes(2);
  });

  test("keeps the contact's devices and your verification as separate facts", () => {
    const cases: Array<[ContactSecuritySummary, string, string]> = [
      [summary({}), "All confirmed by their owner", "Not verified by you"],
      [summary({ identity: "verifiedByYou" }), "All confirmed by their owner", "Verified by you"],
      [unsignedDeviceVerified, "Some not yet confirmed", "Verified by you"]
    ];
    for (const [value, devices, identity] of cases) {
      renderProfile(loaded(value));
      expect(within(row("Their devices")).getByText(devices)).toBeTruthy();
      expect(within(row("Your verification")).getByText(identity)).toBeTruthy();
      cleanup();
    }
  });

  test("routine unconfirmed devices and never-verified contacts stay neutral", () => {
    renderProfile(
      loaded(
        summary({
          devices: "someNotOwnerSigned",
          device_counts: {
            total: 2,
            owner_signed: 1,
            not_owner_signed: 1,
            owner_signature_invalid: 0,
            excluded_dehydrated: 0
          },
          device_signatures: ["ownerSigned", "notOwnerSigned"]
        })
      )
    );
    expect(document.querySelector(".is-attention")).toBeNull();
    expect(document.querySelector("[role='alert']")).toBeNull();
    expect(document.querySelector("[class*='danger']")).toBeNull();
    // No prompt to verify is shown just because verification is not done.
    expect(screen.queryByRole("button", { name: /verify/i })).toBeNull();
  });

  test("an identity change after your verification is a distinct attention state", () => {
    renderProfile(loaded(summary({ identity: "changedAfterVerification" })));
    const identity = row("Your verification");
    expect(identity.classList.contains("is-attention")).toBe(true);
    expect(within(identity).getByText("Identity changed after you verified it")).toBeTruthy();
    expect(row("Their devices").classList.contains("is-attention")).toBe(false);
  });

  test("expanded details explain owner confirmation with counts and ordinal devices", () => {
    const { actions } = renderProfile(loaded(unsignedDeviceVerified));
    const devices = row("Their devices");
    const toggle = within(devices).getByRole("button", { name: "Details: Their devices" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      within(devices).getByText(
        "Some of this person's devices have not been confirmed yet. If you are concerned, ask them to confirm their devices in their app."
      )
    ).toBeTruthy();
    expect(within(devices).getByText("1 of 3 devices confirmed by their owner")).toBeTruthy();
    expect(
      within(devices).getByText("Device signatures that don't match this person's current identity: 1")
    ).toBeTruthy();
    expect(within(devices).getByText("Offline recovery devices not counted: 1")).toBeTruthy();
    expect(within(devices).getByText("Device 3")).toBeTruthy();
    expect(within(devices).getByText("Signature doesn't match")).toBeTruthy();
    // Opening or closing an explanation never dispatches anything.
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(actions.load).toHaveBeenCalledTimes(1);
    expect(actions.close).not.toHaveBeenCalled();

    const identityToggle = within(row("Your verification")).getByRole("button", {
      name: "Details: Your verification"
    });
    fireEvent.click(identityToggle);
    expect(
      screen.getByText(
        "You verified that this account belongs to the person you know. This doesn't confirm devices they haven't confirmed themselves."
      )
    ).toBeTruthy();
    expect(actions.load).toHaveBeenCalledTimes(1);
  });

  test("retrieval failure is shown as unavailable, never as confirmation, and can be retried", () => {
    const { actions } = renderProfile({
      user_id: CONTACT,
      load: { kind: "failed", request_id: 2, failureKind: "network" },
      summary: null
    });
    expect(within(row("Their devices")).getByText("Status unavailable")).toBeTruthy();
    expect(within(row("Your verification")).getByText("Status unavailable")).toBeTruthy();
    expect(screen.queryByText(/confirmed by their owner/)).toBeNull();
    fireEvent.click(within(row("Their devices")).getByRole("button", { name: /Details/ }));
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(actions.load).toHaveBeenCalledTimes(2);
    expect(actions.load).toHaveBeenLastCalledWith(CONTACT);
  });

  test("missing cross-signing and empty device lists are not confirmation", () => {
    renderProfile(
      loaded(
        summary({
          devices: "ownerIdentityMissing",
          device_counts: {
            total: 1,
            owner_signed: 0,
            not_owner_signed: 1,
            owner_signature_invalid: 0,
            excluded_dehydrated: 0
          },
          device_signatures: ["ownerIdentityMissing"],
          identity: "unknown"
        })
      )
    );
    expect(within(row("Their devices")).getByText("Can't be confirmed yet")).toBeTruthy();
    expect(within(row("Your verification")).getByText("Unknown")).toBeTruthy();
    cleanup();

    renderProfile(
      loaded(
        summary({
          devices: "noDevices",
          device_counts: {
            total: 0,
            owner_signed: 0,
            not_owner_signed: 0,
            owner_signature_invalid: 0,
            excluded_dehydrated: 0
          },
          device_signatures: []
        })
      )
    );
    expect(within(row("Their devices")).getByText("No devices found")).toBeTruthy();
    expect(screen.queryByText(/All confirmed/)).toBeNull();
  });

  test("says the details are about keys, not whether the conversation is encrypted", () => {
    renderProfile(loaded(summary({})));
    expect(
      screen.getByText(
        "These details are about this person's keys. They don't show whether a conversation is encrypted."
      )
    ).toBeTruthy();
  });

  test("your own User info shows no contact security details and dispatches nothing", () => {
    const { actions } = renderProfile(loaded(summary({}), ME), ME);
    expect(screen.queryByText("Security")).toBeNull();
    expect(actions.load).not.toHaveBeenCalled();
  });
});
