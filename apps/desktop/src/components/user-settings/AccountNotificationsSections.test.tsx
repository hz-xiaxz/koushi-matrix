// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import {
  type AccountNotificationActions,
  AccountNotificationsLoadStatus,
  EmailNotificationsSection,
  NotificationCategoriesSection
} from "./AccountNotificationsSections";
import { setActiveLocaleProfile } from "../../i18n/messages";
import type { AccountNotificationsSnapshot, AccountNotificationsState } from "../../domain/types";

function actions(): AccountNotificationActions {
  return {
    load: vi.fn(),
    setCategory: vi.fn(),
    setAccountPush: vi.fn(),
    requestEmailToken: vi.fn(),
    resendEmailToken: vi.fn(),
    confirmEmail: vi.fn(),
    submitUia: vi.fn(),
    cancelEmail: vi.fn(),
    enableEmail: vi.fn(),
    disableEmail: vi.fn()
  };
}

function snapshot(overrides: Partial<AccountNotificationsSnapshot> = {}): AccountNotificationsSnapshot {
  return {
    account_push_enabled: true,
    encrypted_event_push: false,
    categories: {
      direct_messages: "on",
      group_messages: "mixed",
      mentions_and_replies: "on",
      invites: "off"
    },
    email_management: "available",
    emails: [],
    unverified_email_pusher_count: 0,
    ...overrides
  };
}

function loaded(
  snap: AccountNotificationsSnapshot,
  extra: Partial<AccountNotificationsState> = {}
): AccountNotificationsState {
  return {
    load: { kind: "loaded" },
    snapshot: snap,
    pending_email: null,
    operation: { kind: "idle" },
    ...extra
  };
}

function renderEmail(state: AccountNotificationsState, handlers = actions()) {
  render(
    <EmailNotificationsSection
      state={state}
      actions={handlers}
      syncKey="session"
      accountManagementAvailable={false}
      onManageAccount={() => undefined}
    />
  );
  return handlers;
}

afterEach(() => {
  cleanup();
  setActiveLocaleProfile("en", "none");
});

describe("NotificationCategoriesSection", () => {
  test("renders the Rust category states and marks mixed without claiming ON", () => {
    const handlers = actions();
    render(<NotificationCategoriesSection state={loaded(snapshot())} actions={handlers} />);
    const dms = screen.getByRole("switch", { name: "Direct messages" });
    const group = screen.getByRole("switch", { name: "Group messages" });
    const invites = screen.getByRole("switch", { name: "Room invites" });
    expect(dms.getAttribute("aria-checked")).toBe("true");
    expect(group.getAttribute("aria-checked")).toBe("false");
    expect(group.getAttribute("data-state")).toBe("mixed");
    expect(within(group).getByText(/Set differently in another app/)).toBeTruthy();
    expect(invites.getAttribute("aria-checked")).toBe("false");

    // Rendering alone dispatches nothing.
    expect(handlers.setCategory).not.toHaveBeenCalled();

    fireEvent.click(group);
    expect(handlers.setCategory).toHaveBeenCalledWith("groupMessages", true);
    fireEvent.click(dms);
    expect(handlers.setCategory).toHaveBeenCalledWith("directMessages", false);
  });

  test("keeps the server value while a toggle is in flight", () => {
    const state = loaded(snapshot(), {
      operation: {
        kind: "working",
        request_id: 3,
        operation: { kind: "setCategory", category: "invites", enabled: true }
      }
    });
    render(<NotificationCategoriesSection state={state} actions={actions()} />);
    const invites = screen.getByRole("switch", { name: "Room invites" });
    expect(invites.getAttribute("aria-checked")).toBe("false");
    expect((invites as HTMLButtonElement).disabled).toBe(true);
  });

  test("offers recovery when another client silenced the account", () => {
    const handlers = actions();
    render(
      <NotificationCategoriesSection
        state={loaded(snapshot({ account_push_enabled: false }))}
        actions={handlers}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Turn on" }));
    expect(handlers.setAccountPush).toHaveBeenCalledWith(true);
  });
});

describe("NotificationCategoriesSection caveats", () => {
  test("warns that encrypted-group mentions reach only this app while group is off", () => {
    render(
      <NotificationCategoriesSection
        state={loaded(
          snapshot({
            categories: {
              direct_messages: "on",
              group_messages: "off",
              mentions_and_replies: "on",
              invites: "on"
            }
          })
        )}
        actions={actions()}
      />
    );
    expect(screen.getByTestId("encrypted-group-caveat").textContent).toMatch(
      /encrypted group rooms your server cannot see mentions/
    );
  });

  test("warns that MSC4028 servers still push encrypted messages", () => {
    render(
      <NotificationCategoriesSection
        state={loaded(
          snapshot({
            encrypted_event_push: true,
            categories: {
              direct_messages: "on",
              group_messages: "off",
              mentions_and_replies: "on",
              invites: "on"
            }
          })
        )}
        actions={actions()}
      />
    );
    expect(screen.getByTestId("encrypted-group-caveat").textContent).toMatch(
      /pushes every encrypted message/
    );
  });

  test("no caveat while group messages are on; unavailable categories are disabled", () => {
    const handlers = actions();
    render(
      <NotificationCategoriesSection
        state={loaded(
          snapshot({
            categories: {
              direct_messages: "on",
              group_messages: "on",
              mentions_and_replies: "on",
              invites: "unavailable"
            }
          })
        )}
        actions={handlers}
      />
    );
    expect(screen.queryByTestId("encrypted-group-caveat")).toBeNull();
    const invites = screen.getByRole("switch", { name: "Room invites" }) as HTMLButtonElement;
    expect(invites.disabled).toBe(true);
    expect(within(invites).getByText("Not available on this server.")).toBeTruthy();
    fireEvent.click(invites);
    expect(handlers.setCategory).not.toHaveBeenCalled();
  });
});

describe("EmailNotificationsSection", () => {
  test("cannot enable email notifications before an address is verified", () => {
    renderEmail(loaded(snapshot()));
    const toggle = screen.getByRole("switch", { name: "Email notifications" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect((toggle as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Add and confirm an email address first")).toBeTruthy();
  });

  test("shows the active pusher target and turns it off", () => {
    const handlers = renderEmail(
      loaded(
        snapshot({
          emails: [
            { address: "one@example.invalid", notifications_active: false },
            { address: "two@example.invalid", notifications_active: true }
          ]
        })
      )
    );
    const toggle = screen.getByRole("switch", { name: "Email notifications" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    // With several addresses the picker shows the target; the switch only says On.
    expect(within(toggle).getByText("On")).toBeTruthy();
    const select = screen.getByTestId("email-notifications-target") as HTMLSelectElement;
    expect(select.value).toBe("two@example.invalid");
    fireEvent.change(select, { target: { value: "one@example.invalid" } });
    expect(handlers.enableEmail).toHaveBeenCalledWith("one@example.invalid");
    fireEvent.click(toggle);
    expect(handlers.disableEmail).toHaveBeenCalled();
  });

  test("enables a verified address and adds a new one through verification", () => {
    const handlers = renderEmail(
      loaded(snapshot({ emails: [{ address: "one@example.invalid", notifications_active: false }] }))
    );
    fireEvent.click(screen.getByRole("switch", { name: "Email notifications" }));
    expect(handlers.enableEmail).toHaveBeenCalledWith("one@example.invalid");

    fireEvent.click(screen.getByRole("button", { name: "Add another email address" }));
    const input = screen.getByTestId("email-address-input");
    fireEvent.change(input, { target: { value: "new@example.invalid" } });
    fireEvent.click(screen.getByRole("button", { name: "Send confirmation email" }));
    expect(handlers.requestEmailToken).toHaveBeenCalledWith("new@example.invalid");
  });

  test("pending verification offers continue, resend, and password re-auth", () => {
    const handlers = actions();
    const pending = { address: "new@example.invalid", resend_count: 1 };
    const { rerender } = render(
      <EmailNotificationsSection
        state={loaded(snapshot(), { pending_email: pending })}
        actions={handlers}
        syncKey="session"
        accountManagementAvailable={false}
        onManageAccount={() => undefined}
      />
    );
    expect(screen.getByText(/We sent a confirmation email to new@example.invalid/)).toBeTruthy();
    expect(screen.getByText("Confirmation email sent again.")).toBeTruthy();
    expect(
      screen.getByRole("switch", { name: "Email notifications" }).getAttribute("aria-checked")
    ).toBe("false");
    fireEvent.click(screen.getByTestId("email-resend"));
    expect(handlers.resendEmailToken).toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("email-confirm"));
    expect(handlers.confirmEmail).toHaveBeenCalled();

    rerender(
      <EmailNotificationsSection
        state={loaded(snapshot(), {
          pending_email: pending,
          operation: {
            kind: "awaitingUia",
            request_id: 7,
            flow_id: 7,
            operation: { kind: "confirmEmail" }
          }
        })}
        actions={handlers}
        syncKey="session"
        accountManagementAvailable={false}
        onManageAccount={() => undefined}
      />
    );
    const password = document.querySelector("input[type=password]") as HTMLInputElement;
    password.value = "synthetic-password";
    fireEvent.input(password);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(handlers.submitUia).toHaveBeenCalledWith(7, "synthetic-password");
  });

  test("explains unsupported servers and failures without showing ON", () => {
    renderEmail(
      loaded(snapshot({ email_management: "unsupported" }), {
        operation: {
          kind: "failed",
          request_id: 2,
          operation: { kind: "enableEmailNotifications" },
          failureKind: "unsupported"
        }
      })
    );
    expect(screen.getByTestId("email-unsupported")).toBeTruthy();
    expect(screen.getByTestId("email-notifications-error").textContent).toBe(
      "This server does not support this."
    );
    expect(screen.queryByTestId("email-add")).toBeNull();
    expect(
      screen.getByRole("switch", { name: "Email notifications" }).getAttribute("aria-checked")
    ).toBe("false");
  });
});

const synthetic = "researcher+matrix@example.test";

function renderEmailWith(
  state: AccountNotificationsState,
  options: { accountManagementAvailable?: boolean; onManageAccount?: () => void } = {}
) {
  const handlers = actions();
  render(
    <EmailNotificationsSection
      state={state}
      actions={handlers}
      syncKey="session"
      accountManagementAvailable={options.accountManagementAvailable ?? false}
      onManageAccount={options.onManageAccount ?? (() => undefined)}
    />
  );
  return handlers;
}

describe("EmailNotificationsSection registered addresses (#1025)", () => {
  test("existing address, notifications off, and unsupported additions read as separate facts", () => {
    const handlers = renderEmailWith(
      loaded(
        snapshot({
          email_management: "unsupported",
          emails: [{ address: synthetic, notifications_active: false }]
        })
      )
    );
    // 1. The address is a registered, confirmed address of the Matrix account.
    const list = screen.getByTestId("notification-email-list");
    expect(screen.getByRole("heading", { name: "Registered email addresses" })).toBeTruthy();
    expect(screen.getByText(/registered with your Matrix account/)).toBeTruthy();
    const row = within(list).getByText(synthetic).closest(".settings-detail-row") as HTMLElement;
    expect(within(row).getByText("Email confirmed")).toBeTruthy();
    // The ambiguous standalone device/contact-style label is gone.
    expect(screen.queryByText("Verified")).toBeNull();

    // 2. Delivery is a separate, still-usable switch that is off.
    const toggle = screen.getByRole("switch", { name: "Email notifications" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect((toggle as HTMLButtonElement).disabled).toBe(false);
    expect(within(toggle).getByText("Off")).toBeTruthy();
    expect(screen.getByText(/does not by itself turn on email notifications/)).toBeTruthy();

    // 3. Only adding is unavailable; the listed address is explained as existing.
    const unsupported = screen.getByTestId("email-unsupported");
    expect(unsupported.textContent).toContain(
      "This server does not allow adding email addresses here."
    );
    expect(unsupported.textContent).toContain("already registered with your account");
    expect(unsupported.textContent).not.toMatch(/remov|chang/i);
    expect(screen.queryByTestId("email-add")).toBeNull();

    fireEvent.click(toggle);
    expect(handlers.enableEmail).toHaveBeenCalledWith(synthetic);
  });

  test("explains why an address is shown without inferring its origin", () => {
    renderEmailWith(
      loaded(snapshot({ emails: [{ address: synthetic, notifications_active: false }] }))
    );
    const why = screen.getByTestId("email-why-shown");
    expect(within(why).getByText("Why is this address shown?")).toBeTruthy();
    expect(why.textContent).toMatch(/not related to device or contact verification/);
    expect(why.textContent).toMatch(/does not mean that mail to it is delivered/);
  });

  test("notifications on name the active target on its row", () => {
    renderEmailWith(
      loaded(snapshot({ emails: [{ address: synthetic, notifications_active: true }] }))
    );
    const toggle = screen.getByRole("switch", { name: "Email notifications" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    expect(within(toggle).getByText(`Sending to ${synthetic}`)).toBeTruthy();
    const list = screen.getByTestId("notification-email-list");
    expect(within(list).getByText("Email confirmed · Notification target")).toBeTruthy();
  });

  test("no registered addresses: empty state and capability-aware prerequisite", () => {
    renderEmailWith(loaded(snapshot()));
    expect(screen.getByText("No email addresses are registered with this account.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add email address" })).toBeTruthy();
    cleanup();

    renderEmailWith(loaded(snapshot({ email_management: "unsupported" })));
    const toggle = screen.getByRole("switch", { name: "Email notifications" });
    expect((toggle as HTMLButtonElement).disabled).toBe(true);
    // Do not tell the user to add an address the server refuses to add here.
    expect(within(toggle).getByText("Needs a registered email address")).toBeTruthy();
    expect(screen.queryByText(/Add and confirm/)).toBeNull();
    expect(screen.getByTestId("email-unsupported").textContent).toBe(
      "This server does not allow adding email addresses here."
    );
  });

  test("multiple registered addresses each show confirmation; only the target is marked", () => {
    const other = "lab+matrix@example.test";
    renderEmailWith(
      loaded(
        snapshot({
          email_management: "unsupported",
          emails: [
            { address: synthetic, notifications_active: false },
            { address: other, notifications_active: true }
          ]
        })
      )
    );
    const list = screen.getByTestId("notification-email-list");
    const rows = list.querySelectorAll(".settings-detail-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByText("Email confirmed")).toBeTruthy();
    expect(
      within(rows[1] as HTMLElement).getByText("Email confirmed · Notification target")
    ).toBeTruthy();
    expect(screen.getByTestId("email-unsupported").textContent).toContain(
      "already registered with your account"
    );
  });

  test("available additions offer adding another address, not replacing one", () => {
    const handlers = renderEmailWith(
      loaded(snapshot({ emails: [{ address: synthetic, notifications_active: false }] }))
    );
    expect(screen.queryByTestId("email-unsupported")).toBeNull();
    expect(screen.queryByRole("button", { name: "Change" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add another email address" }));
    fireEvent.change(screen.getByTestId("email-address-input"), {
      target: { value: "lab+matrix@example.test" }
    });
    fireEvent.click(screen.getByTestId("email-send-verification"));
    expect(handlers.requestEmailToken).toHaveBeenCalledWith("lab+matrix@example.test");
  });

  test("OAuth delegation keeps the account-page explanation and action", () => {
    const onManageAccount = vi.fn();
    renderEmailWith(
      loaded(
        snapshot({
          email_management: "delegatedToAccountManagement",
          emails: [{ address: synthetic, notifications_active: false }]
        })
      ),
      { accountManagementAvailable: true, onManageAccount }
    );
    expect(screen.getByTestId("email-managed-externally").textContent).toContain(
      "managed on your account page"
    );
    expect(screen.queryByTestId("email-unsupported")).toBeNull();
    expect(screen.queryByTestId("email-add")).toBeNull();
    expect(screen.getByText("Email confirmed")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Manage account & devices" }));
    expect(onManageAccount).toHaveBeenCalled();
  });

  test("loading and failed loads claim nothing about addresses", () => {
    const loading: AccountNotificationsState = {
      load: { kind: "loading", request_id: 1 },
      snapshot: null,
      pending_email: null,
      operation: { kind: "idle" }
    };
    renderEmailWith(loading);
    render(<AccountNotificationsLoadStatus state={loading} onRetry={() => undefined} />);
    expect(screen.getByText("Loading notification settings…")).toBeTruthy();
    expect(screen.queryByTestId("notification-email-list")).toBeNull();
    expect(screen.queryByText(/No email addresses are registered/)).toBeNull();
    expect(screen.queryByTestId("email-unsupported")).toBeNull();
    cleanup();

    const onRetry = vi.fn();
    const failed: AccountNotificationsState = {
      ...loading,
      load: { kind: "failed", request_id: 1, failureKind: "network" }
    };
    renderEmailWith(failed);
    render(<AccountNotificationsLoadStatus state={failed} onRetry={onRetry} />);
    expect(screen.getByTestId("account-notifications-load-failed")).toBeTruthy();
    expect(screen.queryByText(/No email addresses are registered/)).toBeNull();
    expect(screen.queryByRole("switch", { name: "Email notifications" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });

  test("Japanese labels use email confirmation, distinct from device verification", () => {
    setActiveLocaleProfile("ja", "none");
    renderEmailWith(
      loaded(
        snapshot({
          email_management: "unsupported",
          emails: [{ address: synthetic, notifications_active: false }]
        })
      )
    );
    expect(screen.getByRole("heading", { name: "登録済みのメールアドレス" })).toBeTruthy();
    const list = screen.getByTestId("notification-email-list");
    expect(within(list).getByText("メールアドレス確認済み")).toBeTruthy();
    // 検証 is reserved for device/contact verification.
    expect(list.textContent).not.toContain("検証");
    expect(screen.getByTestId("email-why-shown").textContent).toContain("デバイスや相手の検証");
    expect(screen.getByTestId("email-unsupported").textContent).toContain(
      "このサーバーでは、ここからメールアドレスを追加できません。"
    );
  });
});
