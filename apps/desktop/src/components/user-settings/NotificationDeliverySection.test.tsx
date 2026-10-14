// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import {
  type NotificationDeliveryActions,
  NotificationDeliverySection
} from "./NotificationDeliverySection";
import { setActiveLocaleProfile } from "../../i18n/messages";
import type {
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationDeliveryState,
  NotificationSinkSettings,
  RoomSummary,
  SpaceSummary
} from "../../domain/types";

const SINK_ID = "sink-11111111-2222-3333-4444-555555555555";

const presets: NotificationDeliveryPresetState[] = [
  {
    id: "gmail",
    label: "Gmail",
    host: "smtp.example.invalid",
    port: 587,
    tls: "startTls",
    username_is_full_address: true,
    secret_hint: "settings.notificationDeliverySecretHint.gmail"
  }
];

const resendSink: NotificationSinkSettings = {
  id: SINK_ID,
  enabled: true,
  destination: "recipient@example.invalid",
  from_address: "sender@example.invalid",
  sender_name: "",
  config: { kind: "resend", endpoint: "https://api.example.invalid/emails" }
};

function delivery(overrides: Partial<NotificationDeliverySettings> = {}): NotificationDeliverySettings {
  return {
    enabled: false,
    direct_messages: true,
    mentions: true,
    keywords: true,
    only_when_unfocused: true,
    dwell_secs: 300,
    min_interval_secs: 900,
    prefix_chars: 0,
    prefix_consent: null,
    watched: [],
    sinks: [resendSink],
    ...overrides
  };
}

function state(overrides: Partial<NotificationDeliveryState> = {}): NotificationDeliveryState {
  return { sinks: [], presets, ...overrides };
}

function actions(): NotificationDeliveryActions {
  return {
    update: vi.fn(),
    setSecret: vi.fn(async () => undefined),
    clearSecret: vi.fn(async () => undefined),
    test: vi.fn(async () => undefined)
  };
}

function rooms(): RoomSummary[] {
  return [
    { room_id: "!alpha:example.invalid", display_label: "Alpha room" } as RoomSummary,
    { room_id: "!beta:example.invalid", display_label: "Beta room" } as RoomSummary
  ];
}

function spaces(): SpaceSummary[] {
  return [{ space_id: "!space:example.invalid", display_name: "Alpha space" } as SpaceSummary];
}

function renderSection({
  delivery: block = delivery(),
  runtime = state(),
  handlers = actions()
}: {
  delivery?: NotificationDeliverySettings;
  runtime?: NotificationDeliveryState;
  handlers?: NotificationDeliveryActions;
} = {}) {
  render(
    <NotificationDeliverySection
      delivery={block}
      state={runtime}
      rooms={rooms()}
      spaces={spaces()}
      actions={handlers}
    />
  );
  return handlers;
}

function lastUpdate(handlers: NotificationDeliveryActions): NotificationDeliverySettings {
  const update = handlers.update as unknown as { mock: { calls: NotificationDeliverySettings[][] } };
  const calls = update.mock.calls;
  return calls[calls.length - 1][0];
}

afterEach(() => {
  cleanup();
  setActiveLocaleProfile("en", "none");
});

describe("NotificationDeliverySection", () => {
  test("masters the channel and the three triggers, and says a keyword hit arrives as a mention", () => {
    const handlers = renderSection();
    // Rendering alone dispatches nothing.
    expect(handlers.update).not.toHaveBeenCalled();

    const enabled = screen.getByTestId("notification-delivery-enabled");
    expect(enabled.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(enabled);
    expect(lastUpdate(handlers).enabled).toBe(true);

    fireEvent.click(screen.getByTestId("notification-delivery-direct-messages"));
    expect(lastUpdate(handlers).direct_messages).toBe(false);
    fireEvent.click(screen.getByTestId("notification-delivery-mentions"));
    expect(lastUpdate(handlers).mentions).toBe(false);
    fireEvent.click(screen.getByTestId("notification-delivery-keywords"));
    expect(lastUpdate(handlers).keywords).toBe(false);

    expect(screen.getByTestId("notification-delivery-keywords-hint").textContent).toContain("mention");
  });

  test("bounds the dwell and the interval at the Rust maximum", () => {
    const handlers = renderSection();
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    fireEvent.change(dwell, { target: { value: "999999" } });
    fireEvent.blur(dwell);
    expect(lastUpdate(handlers).dwell_secs).toBe(86_400);

    const interval = screen.getByTestId("notification-delivery-interval") as HTMLInputElement;
    fireEvent.change(interval, { target: { value: "-4" } });
    fireEvent.blur(interval);
    expect(lastUpdate(handlers).min_interval_secs).toBe(0);
  });

  test("shows the bound the settings now hold instead of the entry Rust rejected", () => {
    const handlers = actions();
    const { rerender } = render(
      <NotificationDeliverySection
        delivery={delivery()}
        state={state()}
        rooms={rooms()}
        spaces={spaces()}
        actions={handlers}
      />
    );
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    fireEvent.change(dwell, { target: { value: "999999" } });
    fireEvent.blur(dwell);
    expect(lastUpdate(handlers).dwell_secs).toBe(86_400);

    // The committed settings block comes back at the bound; leaving the rejected entry on
    // screen would describe a dwell the device does not have.
    rerender(
      <NotificationDeliverySection
        delivery={delivery({ dwell_secs: 86_400 })}
        state={state()}
        rooms={rooms()}
        spaces={spaces()}
        actions={handlers}
      />
    );
    expect((screen.getByTestId("notification-delivery-dwell") as HTMLInputElement).value).toBe(
      "86400"
    );
  });

  test("names each watched entry from the room list, adds one and removes one", () => {
    const handlers = renderSection({
      delivery: delivery({ watched: [{ kind: "room", room_id: "!alpha:example.invalid" }] })
    });
    const rows = screen.getAllByTestId("notification-delivery-watched-row");
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain("Alpha room");

    fireEvent.click(within(rows[0]).getByTestId("notification-delivery-watched-remove"));
    expect(lastUpdate(handlers).watched).toEqual([]);

    fireEvent.change(screen.getByTestId("notification-delivery-watched-add"), {
      target: { value: "space:!space:example.invalid" }
    });
    expect(lastUpdate(handlers).watched).toEqual([
      { kind: "room", room_id: "!alpha:example.invalid" },
      { kind: "space", space_id: "!space:example.invalid" }
    ]);
  });

  test("reports a watched entry the room list no longer has without printing its id", () => {
    renderSection({
      delivery: delivery({ watched: [{ kind: "room", room_id: "!gone:example.invalid" }] })
    });
    const row = screen.getByTestId("notification-delivery-watched-row");
    expect(row.textContent).toContain("Not in the room list right now");
    expect(row.textContent).not.toContain("!gone:example.invalid");
  });

  test("keeps a sink's secret write-only and dispatches the set and clear commands", async () => {
    const handlers = renderSection({
      runtime: state({
        sinks: [{ sink_id: SINK_ID, secret_configured: true, last_outcome: null }]
      })
    });
    const secret = screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement;
    // A stored secret is reported as configured, never rendered back into the field.
    expect(secret.value).toBe("");
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain(
      "Saved"
    );

    fireEvent.change(secret, { target: { value: "synthetic-app-password" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("notification-delivery-sink-save-secret"));
    });
    expect(handlers.setSecret).toHaveBeenCalledWith(SINK_ID, "synthetic-app-password");
    await waitFor(() => expect(secret.value).toBe(""));

    fireEvent.click(screen.getByTestId("notification-delivery-sink-clear-secret"));
    expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID);
  });

  test("a sink whose last send failed does not read as enabled and working", () => {
    renderSection({
      runtime: state({
        sinks: [
          {
            sink_id: SINK_ID,
            secret_configured: true,
            last_outcome: { kind: "invalidSecret", at_ms: 1 }
          }
        ]
      })
    });
    const card = screen.getByTestId("notification-delivery-sink");
    expect(card.getAttribute("data-state")).toBe("failed");
    expect(screen.getByTestId("notification-delivery-sink-failed")).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain(
      "rejected the key"
    );
    // The switch still shows Rust's enabled value; the user can turn it off.
    expect(screen.getByTestId("notification-delivery-sink-enabled").getAttribute("aria-checked")).toBe(
      "true"
    );
  });

  test("names the wait when the service rate limited the send", () => {
    renderSection({
      runtime: state({
        sinks: [
          {
            sink_id: SINK_ID,
            secret_configured: true,
            last_outcome: { kind: "rateLimited", at_ms: 1, retry_after_secs: 120 }
          }
        ]
      })
    });
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain("120 seconds");
  });

  test("sends one test message through the typed command", async () => {
    const handlers = renderSection({
      runtime: state({
        sinks: [{ sink_id: SINK_ID, secret_configured: true, last_outcome: null }]
      })
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("notification-delivery-sink-test"));
    });
    expect(handlers.test).toHaveBeenCalledWith(SINK_ID);
  });

  test("turns the plaintext prefix on only after the consent dialog is acknowledged", () => {
    const handlers = renderSection({ runtime: state({ presets }) });
    const prefix = screen.getByTestId("notification-delivery-sink-prefix");
    expect(prefix.getAttribute("aria-checked")).toBe("false");

    fireEvent.click(prefix);
    const accept = screen.getByTestId("notification-delivery-consent-accept") as HTMLButtonElement;
    expect(accept.disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-consent-body").textContent).toContain(
      "without end-to-end encryption"
    );
    expect(handlers.update).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("notification-delivery-consent-acknowledge"));
    expect(accept.disabled).toBe(false);
    fireEvent.click(accept);

    const patch = lastUpdate(handlers);
    expect(patch.prefix_chars).toBe(80);
    expect(patch.prefix_consent).toMatchObject({
      sink_id: SINK_ID,
      destination: resendSink.destination,
      endpoint: "https://api.example.invalid/emails"
    });
  });

  test("closes the consent dialog without turning the prefix on", () => {
    const handlers = renderSection();
    fireEvent.click(screen.getByTestId("notification-delivery-sink-prefix"));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(handlers.update).not.toHaveBeenCalled();
    expect(screen.queryByTestId("notification-delivery-consent-accept")).toBeNull();
  });

  test("reads the prefix as off for a sink whose stored consent names another destination", () => {
    renderSection({
      delivery: delivery({
        prefix_chars: 80,
        prefix_consent: {
          sink_id: SINK_ID,
          destination: "somebody-else@example.invalid",
          endpoint: "https://api.example.invalid/emails",
          accepted_at_ms: 1
        }
      })
    });
    expect(screen.getByTestId("notification-delivery-sink-prefix").getAttribute("aria-checked")).toBe(
      "false"
    );
    expect(screen.getByTestId("notification-delivery-sink-prefix-required")).toBeTruthy();
  });

  test("turning the prefix off forgets the consent", () => {
    const handlers = renderSection({
      delivery: delivery({
        prefix_chars: 80,
        prefix_consent: {
          sink_id: SINK_ID,
          destination: resendSink.destination,
          endpoint: "https://api.example.invalid/emails",
          accepted_at_ms: 1
        }
      })
    });
    fireEvent.click(screen.getByTestId("notification-delivery-sink-prefix"));
    const patch = lastUpdate(handlers);
    expect(patch.prefix_chars).toBe(0);
    expect(patch.prefix_consent).toBeNull();
  });

  test("changes the prefix character count within the Rust bound", () => {
    const handlers = renderSection({
      delivery: delivery({
        prefix_chars: 80,
        prefix_consent: {
          sink_id: SINK_ID,
          destination: resendSink.destination,
          endpoint: "https://api.example.invalid/emails",
          accepted_at_ms: 1
        }
      })
    });
    const chars = screen.getByTestId("notification-delivery-sink-prefix-chars") as HTMLInputElement;
    fireEvent.change(chars, { target: { value: "5000" } });
    fireEvent.blur(chars);
    expect(lastUpdate(handlers).prefix_chars).toBe(200);
    expect(lastUpdate(handlers).prefix_consent?.sink_id).toBe(SINK_ID);
  });

  test("adds a complete sink from the add form", () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }), runtime: state({ presets }) });
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();

    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    const confirm = screen.getByTestId("notification-delivery-add-confirm") as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);

    fireEvent.change(screen.getByTestId("notification-delivery-add-kind"), {
      target: { value: "smtp" }
    });
    fireEvent.change(screen.getByTestId("notification-delivery-add-from"), {
      target: { value: "sender@example.invalid" }
    });
    fireEvent.change(screen.getByTestId("notification-delivery-add-to"), {
      target: { value: "recipient@example.invalid" }
    });
    fireEvent.change(screen.getByTestId("notification-delivery-add-preset"), {
      target: { value: "gmail" }
    });
    // The preset fills the relay the provider documents, and the username keeps its shape.
    expect((screen.getByTestId("notification-delivery-add-host") as HTMLInputElement).value).toBe(
      "smtp.example.invalid"
    );
    expect((screen.getByTestId("notification-delivery-add-port") as HTMLInputElement).value).toBe("587");
    expect((screen.getByTestId("notification-delivery-add-tls") as HTMLSelectElement).value).toBe(
      "startTls"
    );
    expect((screen.getByTestId("notification-delivery-add-username") as HTMLInputElement).value).toBe(
      "sender@example.invalid"
    );
    expect(screen.getByTestId("notification-delivery-add-secret-hint").textContent).toContain(
      "app password"
    );

    expect(confirm.disabled).toBe(false);
    fireEvent.click(confirm);
    const patch = lastUpdate(handlers);
    expect(patch.sinks).toHaveLength(1);
    expect(patch.sinks[0].id).toMatch(/^sink-[a-z0-9-]+$/);
    expect(patch.sinks[0]).toMatchObject({
      enabled: false,
      destination: "recipient@example.invalid",
      from_address: "sender@example.invalid",
      config: {
        kind: "smtp",
        preset: "gmail",
        host: "smtp.example.invalid",
        port: 587,
        tls: "startTls",
        username: "sender@example.invalid"
      }
    });
  });

  test("changes an SMTP sink's own fields and its preset", () => {
    const smtpSink: NotificationSinkSettings = {
      ...resendSink,
      config: {
        kind: "smtp",
        preset: null,
        host: "smtp.example.invalid",
        port: 465,
        tls: "implicit",
        username: "sender@example.invalid"
      }
    };
    const handlers = renderSection({ delivery: delivery({ sinks: [smtpSink] }) });
    const host = screen.getByTestId("notification-delivery-sink-host") as HTMLInputElement;
    fireEvent.change(host, { target: { value: "relay.example.invalid" } });
    fireEvent.blur(host);
    const patch = lastUpdate(handlers);
    expect(patch.sinks[0].config).toMatchObject({ host: "relay.example.invalid", port: 465 });

    fireEvent.change(screen.getByTestId("notification-delivery-sink-preset"), {
      target: { value: "gmail" }
    });
    expect(lastUpdate(handlers).sinks[0].config).toMatchObject({
      preset: "gmail",
      host: "smtp.example.invalid",
      port: 587,
      tls: "startTls"
    });
  });

  test("removes a sink and releases the credential it was keyed by", () => {
    const handlers = renderSection({
      runtime: state({
        sinks: [{ sink_id: SINK_ID, secret_configured: true, last_outcome: null }]
      })
    });
    fireEvent.click(screen.getByTestId("notification-delivery-sink-remove"));
    expect(lastUpdate(handlers).sinks).toEqual([]);
    expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID);
  });

  test("stops offering a ninth service once Rust would drop it", () => {
    const sinks = Array.from({ length: 8 }, (_, index) => ({
      ...resendSink,
      id: `sink-${index}`
    }));
    renderSection({ delivery: delivery({ sinks }) });
    const add = screen.getByTestId("notification-delivery-add") as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-sinks-limit").textContent).toContain("8");
  });

  test("stops offering a watch entry once the list is at the Rust cap", () => {
    const watched = Array.from({ length: 64 }, (_, index) => ({
      kind: "room" as const,
      room_id: `!room-${index}:example.invalid`
    }));
    renderSection({ delivery: delivery({ watched }) });
    const add = screen.getByTestId("notification-delivery-watched-add") as HTMLSelectElement;
    expect(add.disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-watched-limit").textContent).toContain("64");
  });

  test("ships the same switches in Japanese", () => {
    setActiveLocaleProfile("ja", "none");
    renderSection({
      runtime: state({
        sinks: [
          {
            sink_id: SINK_ID,
            secret_configured: false,
            last_outcome: { kind: "transport", at_ms: 1 }
          }
        ]
      })
    });
    expect(screen.getByRole("switch", { name: "この端末から通知を送信する" })).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain(
      "接続できませんでした"
    );
  });
});
