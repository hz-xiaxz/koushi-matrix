// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";

import {
  type NotificationDeliveryActions,
  type NotificationKeywordActions,
  NotificationDeliverySection
} from "./NotificationDeliverySection";
import { setActiveLocaleProfile } from "../../i18n/messages";
import type {
  AccountNotificationsOperationState,
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationDeliverySinkState,
  NotificationDeliveryState,
  NotificationKeywordRule,
  NotificationSinkSettings,
  RoomSummary,
  SpaceSummary
} from "../../domain/types";

const SINK_ID = "sink-11111111-2222-3333-4444-555555555555";
const SC_SINK_ID = "sink-22222222-3333-4444-5555-666666666666";

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

const baseSink: Omit<NotificationSinkSettings, "id" | "config"> = {
  destination: "recipient@example.invalid",
  from_address: "sender@example.invalid",
  sender_name: "",
  secret_binding: null,
  targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
  content: { kind: "namesOnly" },
  content_consent: null,
  timing: { dwell_secs: 300, min_interval_secs: 900, only_when_unfocused: true }
};

const resendSink: NotificationSinkSettings = {
  ...baseSink,
  id: SINK_ID,
  config: { kind: "resend", endpoint: "https://api.example.invalid/emails" }
};

const smtpSink: NotificationSinkSettings = {
  ...resendSink,
  config: {
    kind: "smtp",
    preset: "gmail",
    host: "smtp.example.invalid",
    port: 465,
    tls: "implicit",
    username: "sender@example.invalid"
  }
};

const serverChanSink: NotificationSinkSettings = {
  ...baseSink,
  id: SC_SINK_ID,
  destination: "",
  from_address: "",
  config: { kind: "serverChan", endpoint: "https://sctapi.ftqq.com" }
};

function delivery(overrides: Partial<NotificationDeliverySettings> = {}): NotificationDeliverySettings {
  return { sinks: [resendSink], ...overrides };
}

function state(overrides: Partial<NotificationDeliveryState> = {}): NotificationDeliveryState {
  return { sinks: [], presets, ...overrides };
}

function sinkState(overrides: Partial<NotificationDeliverySinkState> = {}): NotificationDeliverySinkState {
  return {
    sink_id: SINK_ID,
    secret_configured: true,
    secret_retired: false,
    last_outcome: null,
    ...overrides
  };
}

function actions(): NotificationDeliveryActions {
  return {
    update: vi.fn(),
    setSecret: vi.fn(async () => undefined),
    clearSecret: vi.fn(async () => undefined),
    test: vi.fn(async () => undefined)
  };
}

const KEYWORD_RULES: NotificationKeywordRule[] = [
  {
    rule_id: "org.matrix.desktop.notify.keyword.aaaaaaaaaaaaaaaa",
    pattern: "Alpha",
    enabled: true
  },
  {
    // Another client turned this rule off; it is still on the account, so it
    // still appears in server order and is still removable.
    rule_id: "org.matrix.desktop.notify.keyword.bbbbbbbbbbbbbbbb",
    pattern: "Beta",
    enabled: false
  }
];

function keywordActions(): NotificationKeywordActions {
  return { add: vi.fn(), remove: vi.fn() };
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

type SectionProps = {
  delivery?: NotificationDeliverySettings;
  runtime?: NotificationDeliveryState;
  handlers?: NotificationDeliveryActions;
  keywordRules?: readonly NotificationKeywordRule[];
  keywordOperation?: AccountNotificationsOperationState;
  keywordHandlers?: NotificationKeywordActions;
};

function section({
  delivery: block = delivery(),
  runtime = state(),
  handlers = actions(),
  keywordRules = KEYWORD_RULES,
  keywordOperation = { kind: "idle" },
  keywordHandlers = keywordActions()
}: SectionProps = {}) {
  return (
    <NotificationDeliverySection
      delivery={block}
      state={runtime}
      rooms={rooms()}
      spaces={spaces()}
      actions={handlers}
      keywordRules={keywordRules}
      keywordOperation={keywordOperation}
      keywordActions={keywordHandlers}
    />
  );
}

function renderSection(props: SectionProps = {}) {
  const handlers = props.handlers ?? actions();
  render(section({ ...props, handlers }));
  return handlers;
}

function renderKeywordSection(props: SectionProps = {}) {
  const handlers = props.handlers ?? actions();
  const keywordHandlers = props.keywordHandlers ?? keywordActions();
  render(section({ ...props, handlers, keywordHandlers }));
  return { handlers, keywordHandlers };
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

/** Opens the detail view of the sink whose list row carries `sinkId`. */
function openSink(sinkId: string = SINK_ID) {
  const row = screen
    .getAllByTestId("notification-delivery-sink")
    .find((candidate) => candidate.getAttribute("data-sink-id") === sinkId);
  if (!row) {
    throw new Error(`no list row for ${sinkId}`);
  }
  fireEvent.click(row);
}

function lastSink(handlers: NotificationDeliveryActions, sinkId: string = SINK_ID): NotificationSinkSettings {
  const sink = lastUpdate(handlers).sinks.find((candidate) => candidate.id === sinkId);
  if (!sink) {
    throw new Error(`no ${sinkId} in the last patch`);
  }
  return sink;
}

describe("NotificationDeliverySection list", () => {
  test("lists each destination as Configured or Not configured with no enable switch", () => {
    const handlers = renderSection({
      delivery: delivery({ sinks: [resendSink, serverChanSink] }),
      runtime: state({ sinks: [sinkState()] })
    });
    expect(handlers.update).not.toHaveBeenCalled();
    const rows = screen.getAllByTestId("notification-delivery-sink");
    expect(rows).toHaveLength(2);
    expect(rows[0].getAttribute("data-setup")).toBe("configured");
    expect(within(rows[0]).getByTestId("notification-delivery-sink-status").textContent).toBe("Configured");
    expect(rows[0].textContent).toContain("recipient@example.invalid");
    expect(rows[1].getAttribute("data-setup")).toBe("needsCredential");
    expect(within(rows[1]).getByTestId("notification-delivery-sink-status").textContent).toBe(
      "Not configured"
    );
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.getByTestId("notification-delivery-running-note").textContent).toContain("running");
  });

  test("marks a destination whose last send failed", () => {
    renderSection({
      runtime: state({ sinks: [sinkState({ last_outcome: { kind: "invalidSecret", at_ms: 1 } })] })
    });
    const row = screen.getByTestId("notification-delivery-sink");
    expect(row.getAttribute("data-state")).toBe("failed");
    expect(within(row).getByTestId("notification-delivery-sink-failed")).toBeTruthy();
  });

  test("opens a destination's detail and returns with Back", () => {
    renderSection();
    openSink();
    const detail = screen.getByTestId("notification-delivery-detail");
    expect(detail.getAttribute("data-draft")).toBe("false");
    for (const part of ["credentials", "targets", "content", "timing", "outcome"]) {
      expect(within(detail).getByTestId(`notification-delivery-${part}`)).toBeTruthy();
    }
    fireEvent.click(screen.getByTestId("notification-delivery-back"));
    expect(screen.queryByTestId("notification-delivery-detail")).toBeNull();
    expect(screen.getByTestId("notification-delivery-sink")).toBeTruthy();
  });

  test("stops offering a ninth destination once Rust would drop it", () => {
    const sinks = Array.from({ length: 8 }, (_, index) => ({ ...resendSink, id: `sink-${index}` }));
    renderSection({ delivery: delivery({ sinks }) });
    expect((screen.getByTestId("notification-delivery-add") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-sinks-limit").textContent).toContain("8");
  });
});

describe("NotificationDeliverySection add flow", () => {
  test("adds an SMTP destination through the same detail view, credential after save", () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();

    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    const kinds = screen.getAllByTestId("notification-delivery-pick-kind");
    expect(kinds.map((kind) => kind.getAttribute("data-kind"))).toEqual(["resend", "smtp", "serverChan"]);
    fireEvent.click(kinds[1]);

    const detail = screen.getByTestId("notification-delivery-detail");
    expect(detail.getAttribute("data-draft")).toBe("true");
    // The credential store is keyed by a saved sink, so the field waits for the save.
    expect(screen.queryByTestId("notification-delivery-sink-secret")).toBeNull();
    expect(screen.getByTestId("notification-delivery-secret-after-save")).toBeTruthy();
    // Content consent binds to a saved destination too.
    expect(screen.queryByTestId("notification-delivery-content")).toBeNull();
    const save = screen.getByTestId("notification-delivery-draft-save") as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    expect(handlers.update).not.toHaveBeenCalled();

    fireEvent.change(screen.getByTestId("notification-delivery-sink-from"), {
      target: { value: " sender@example.invalid " }
    });
    fireEvent.change(screen.getByTestId("notification-delivery-sink-to"), {
      target: { value: "recipient@example.invalid" }
    });
    fireEvent.change(screen.getByTestId("notification-delivery-sink-preset"), {
      target: { value: "gmail" }
    });
    expect((screen.getByTestId("notification-delivery-sink-port") as HTMLInputElement).value).toBe("587");
    expect(screen.getByTestId("notification-delivery-add-secret-hint").textContent).toContain(
      "app password"
    );
    // Drafting dispatches nothing until the explicit save.
    expect(handlers.update).not.toHaveBeenCalled();
    expect(save.disabled).toBe(false);
    fireEvent.click(save);

    const patch = lastUpdate(handlers);
    expect(patch.sinks).toHaveLength(1);
    expect(patch.sinks[0].id).toMatch(/^sink-[a-z0-9-]+$/);
    expect(patch.sinks[0]).toMatchObject({
      destination: "recipient@example.invalid",
      from_address: "sender@example.invalid",
      secret_binding: null,
      targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
      content: { kind: "namesOnly" },
      content_consent: null,
      timing: { dwell_secs: 300, min_interval_secs: 900, only_when_unfocused: true },
      config: { kind: "smtp", preset: "gmail", host: "smtp.example.invalid", port: 587, tls: "startTls" }
    });
    // The view stays on the saved destination, now with its credential field.
    expect(screen.getByTestId("notification-delivery-detail").getAttribute("data-draft")).toBe("false");
    expect(screen.getByTestId("notification-delivery-sink-secret")).toBeTruthy();
  });

  test("cancelling a draft adds nothing", () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    fireEvent.click(screen.getAllByTestId("notification-delivery-pick-kind")[0]);
    fireEvent.click(screen.getByTestId("notification-delivery-draft-cancel"));
    expect(handlers.update).not.toHaveBeenCalled();
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();
  });

  test("a ServerChan destination can be saved at once and explains the setup", () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    fireEvent.click(screen.getAllByTestId("notification-delivery-pick-kind")[2]);

    const steps = screen.getByTestId("notification-delivery-serverchan-steps");
    expect(steps.querySelectorAll("li")).toHaveLength(3);
    expect(steps.textContent).toContain("SendKey");
    expect(steps.textContent).toContain("SCT");
    expect(screen.getByTestId("notification-delivery-serverchan-link").textContent).toContain(
      "sct.ftqq.com"
    );
    expect(screen.getByTestId("notification-delivery-serverchan-limits")).toBeTruthy();
    expect(screen.queryByTestId("notification-delivery-sink-from")).toBeNull();

    fireEvent.click(screen.getByTestId("notification-delivery-draft-save"));
    expect(lastUpdate(handlers).sinks[0].config).toEqual({
      kind: "serverChan",
      endpoint: "https://sctapi.ftqq.com"
    });
  });
});

describe("NotificationDeliverySection credentials", () => {
  test("keeps a Resend API key write-only and says where From must live", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    expect(screen.getByTestId("notification-delivery-resend-hint").textContent).toContain("verified");
    const secret = screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement;
    expect(secret.getAttribute("aria-label")).toBe("Resend API key");
    expect(secret.getAttribute("autocomplete")).toBe("off");
    expect(secret.value).toBe("");
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain("Saved");

    fireEvent.change(secret, { target: { value: "synthetic-api-key" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("notification-delivery-sink-save-secret"));
    });
    expect(handlers.setSecret).toHaveBeenCalledWith(SINK_ID, "synthetic-api-key");
    await waitFor(() => expect(secret.value).toBe(""));
    // The value never reaches the rendered page.
    expect(document.body.innerHTML).not.toContain("synthetic-api-key");

    fireEvent.click(screen.getByTestId("notification-delivery-sink-clear-secret"));
    expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID);
  });

  test("refuses a ServerChan3 sctp key before it leaves and accepts an SCT SendKey", async () => {
    const handlers = renderSection({
      delivery: delivery({ sinks: [serverChanSink] }),
      runtime: state({ sinks: [sinkState({ sink_id: SC_SINK_ID, secret_configured: false })] })
    });
    openSink(SC_SINK_ID);
    const secret = screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement;
    expect(secret.getAttribute("aria-label")).toBe("SendKey");

    fireEvent.change(secret, { target: { value: "sctp123tSYNTHETIC" } });
    const problem = screen.getByTestId("notification-delivery-sink-secret-problem");
    expect(problem.textContent).toContain("sctp");
    expect(problem.textContent).not.toContain("SYNTHETIC");
    expect((screen.getByTestId("notification-delivery-sink-save-secret") as HTMLButtonElement).disabled).toBe(
      true
    );

    fireEvent.change(secret, { target: { value: " SCTsynthetic123 " } });
    expect(screen.queryByTestId("notification-delivery-sink-secret-problem")).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByTestId("notification-delivery-sink-save-secret"));
    });
    expect(handlers.setSecret).toHaveBeenCalledWith(SC_SINK_ID, "SCTsynthetic123");
  });

  test("says why a stored password is gone when Rust retired it", () => {
    const boundSink = { ...smtpSink, secret_binding: "smtp:smtp.example.invalid:465:sender@example.invalid#1" };
    const handlers = actions();
    const { rerender } = render(
      section({ delivery: delivery({ sinks: [boundSink] }), runtime: state({ sinks: [sinkState()] }), handlers })
    );
    openSink();
    expect(screen.queryByTestId("notification-delivery-sink-secret-retired")).toBeNull();

    const field = screen.getByTestId("notification-delivery-sink-host") as HTMLInputElement;
    fireEvent.change(field, { target: { value: "smtp.other.invalid" } });
    fireEvent.blur(field);
    // The binding goes back to Rust untouched; Rust is what decides the credential is stale.
    expect(lastSink(handlers).secret_binding).toBe(boundSink.secret_binding);

    rerender(
      section({
        delivery: delivery({ sinks: [{ ...lastSink(handlers), secret_binding: null }] }),
        runtime: state({ sinks: [sinkState({ secret_configured: false, secret_retired: true })] }),
        handlers
      })
    );
    const explanation = screen.getByTestId("notification-delivery-sink-secret-retired");
    expect(explanation.textContent).toContain("server or account changed");
    expect(explanation.tagName).toBe("P");
  });

  test("never reads a configured sink as having had its password removed", () => {
    renderSection({ runtime: state({ sinks: [sinkState({ secret_retired: true })] }) });
    openSink();
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain("Saved.");
    expect(screen.queryByTestId("notification-delivery-sink-secret-retired")).toBeNull();
  });

  test("never retires the password for a destination, name or from address change", () => {
    const boundSink = { ...resendSink, secret_binding: "resend:https://api.example.invalid/emails#1" };
    const handlers = renderSection({
      delivery: delivery({ sinks: [boundSink] }),
      runtime: state({ sinks: [sinkState()] })
    });
    openSink();
    for (const [testId, value] of [
      ["notification-delivery-sink-from", "other@example.invalid"],
      ["notification-delivery-sink-name", "Another name"],
      ["notification-delivery-sink-to", "elsewhere@example.invalid"]
    ] as const) {
      const field = screen.getByTestId(testId) as HTMLInputElement;
      fireEvent.change(field, { target: { value } });
      fireEvent.blur(field);
    }
    expect(handlers.clearSecret).not.toHaveBeenCalled();
    expect(handlers.setSecret).not.toHaveBeenCalled();
    const calls = (handlers.update as unknown as { mock: { calls: NotificationDeliverySettings[][] } }).mock
      .calls;
    expect(calls.length).toBe(3);
    for (const call of calls) {
      expect(call[0].sinks[0].secret_binding).toBe(boundSink.secret_binding);
    }
  });

  test("keeps the endpoint override under Advanced", () => {
    renderSection();
    openSink();
    const advanced = screen.getByTestId("notification-delivery-advanced");
    expect(advanced.tagName).toBe("DETAILS");
    expect(within(advanced).getByTestId("notification-delivery-sink-endpoint")).toBeTruthy();
  });

  test("changes an SMTP sink's own fields and its preset", () => {
    const handlers = renderSection({
      delivery: delivery({ sinks: [{ ...smtpSink, config: { ...smtpSink.config, preset: null } as NotificationSinkSettings["config"] }] })
    });
    openSink();
    const host = screen.getByTestId("notification-delivery-sink-host") as HTMLInputElement;
    fireEvent.change(host, { target: { value: "relay.example.invalid" } });
    fireEvent.blur(host);
    expect(lastSink(handlers).config).toMatchObject({ host: "relay.example.invalid", port: 465 });
  });
});

describe("NotificationDeliverySection targets", () => {
  test("toggles the categories for this destination only", () => {
    const other = { ...resendSink, id: "sink-other" };
    const handlers = renderSection({ delivery: delivery({ sinks: [resendSink, other] }) });
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-direct-messages"));
    expect(lastSink(handlers).targets.direct_messages).toBe(false);
    expect(lastSink(handlers, "sink-other").targets.direct_messages).toBe(true);
    expect(screen.getByTestId("notification-delivery-keywords-hint").textContent).toContain("homeserver");
  });

  test("names each watched entry from the room list, adds one and removes one", () => {
    const handlers = renderSection({
      delivery: delivery({
        sinks: [
          {
            ...resendSink,
            targets: { ...resendSink.targets, watched: [{ kind: "room", room_id: "!alpha:example.invalid" }] }
          }
        ]
      })
    });
    openSink();
    expect(screen.getByTestId("notification-delivery-watched-description").textContent).toContain(
      "added later"
    );
    const rows = screen.getAllByTestId("notification-delivery-watched-row");
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain("Alpha room");

    fireEvent.click(within(rows[0]).getByTestId("notification-delivery-watched-remove"));
    expect(lastSink(handlers).targets.watched).toEqual([]);

    fireEvent.change(screen.getByTestId("notification-delivery-watched-add"), {
      target: { value: "space:!space:example.invalid" }
    });
    expect(lastSink(handlers).targets.watched).toEqual([
      { kind: "room", room_id: "!alpha:example.invalid" },
      { kind: "space", space_id: "!space:example.invalid" }
    ]);
  });

  test("reports a watched entry the room list no longer has without printing its id", () => {
    renderSection({
      delivery: delivery({
        sinks: [
          {
            ...resendSink,
            targets: { ...resendSink.targets, watched: [{ kind: "room", room_id: "!gone:example.invalid" }] }
          }
        ]
      })
    });
    openSink();
    const row = screen.getByTestId("notification-delivery-watched-row");
    expect(row.textContent).toContain("Not in the room list right now");
    expect(row.textContent).not.toContain("!gone:example.invalid");
  });

  test("stops offering a watch entry once the list is at the Rust cap", () => {
    const watched = Array.from({ length: 64 }, (_, index) => ({
      kind: "room" as const,
      room_id: `!room-${index}:example.invalid`
    }));
    renderSection({
      delivery: delivery({ sinks: [{ ...resendSink, targets: { ...resendSink.targets, watched } }] })
    });
    openSink();
    expect((screen.getByTestId("notification-delivery-watched-add") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-watched-limit").textContent).toContain("64");
  });
});

describe("NotificationDeliverySection content", () => {
  function mode(name: string): HTMLInputElement {
    const radio = screen
      .getAllByTestId("notification-delivery-content-mode")
      .find((candidate) => candidate.getAttribute("data-mode") === name);
    if (!radio) {
      throw new Error(`no ${name} mode`);
    }
    return radio as HTMLInputElement;
  }

  test("starts at names only and says the provider receives plaintext only once text is on", () => {
    renderSection();
    openSink();
    expect(mode("namesOnly").checked).toBe(true);
    expect(screen.queryByTestId("notification-delivery-content-plaintext")).toBeNull();
    expect(screen.getByTestId("notification-delivery-content-fallback").textContent).toContain(
      "names only"
    );
    expect(screen.getByTestId("notification-delivery-content-independent")).toBeTruthy();
    // The full mode states its bound rather than promising the whole message.
    expect(mode("full").parentElement?.textContent).toContain("4000");
  });

  test("turns a text mode on only after the consent dialog is acknowledged", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    try {
      const handlers = renderSection();
      openSink();
      fireEvent.click(mode("prefix"));
      expect(handlers.update).not.toHaveBeenCalled();
      expect(screen.getByTestId("notification-delivery-consent-body").textContent).toContain(
        "recipient@example.invalid"
      );
      const accept = screen.getByTestId("notification-delivery-consent-accept") as HTMLButtonElement;
      expect(accept.disabled).toBe(true);
      fireEvent.click(screen.getByTestId("notification-delivery-consent-acknowledge"));
      fireEvent.click(accept);
      expect(lastSink(handlers).content).toEqual({ kind: "prefix", graphemes: 80 });
      expect(lastSink(handlers).content_consent).toEqual({
        sink_kind: "resend",
        endpoint: "https://api.example.invalid/emails",
        recipient: "recipient@example.invalid",
        accepted_at_ms: 1_700_000_000_000
      });
    } finally {
      now.mockRestore();
    }
  });

  test("closing the consent dialog leaves names only", () => {
    const handlers = renderSection();
    openSink();
    fireEvent.click(mode("full"));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(handlers.update).not.toHaveBeenCalled();
    expect(mode("namesOnly").checked).toBe(true);
  });

  test("reads a consent for another recipient as names only", () => {
    renderSection({
      delivery: delivery({
        sinks: [
          {
            ...resendSink,
            content: { kind: "full" },
            content_consent: {
              sink_kind: "resend",
              endpoint: "https://api.example.invalid/emails",
              recipient: "other@example.invalid",
              accepted_at_ms: 1
            }
          }
        ]
      })
    });
    openSink();
    expect(mode("namesOnly").checked).toBe(true);
    expect(screen.queryByTestId("notification-delivery-content-plaintext")).toBeNull();
  });

  test("switching back to names only forgets the consent", () => {
    const consented: NotificationSinkSettings = {
      ...resendSink,
      content: { kind: "prefix", graphemes: 40 },
      content_consent: {
        sink_kind: "resend",
        endpoint: "https://api.example.invalid/emails",
        recipient: "recipient@example.invalid",
        accepted_at_ms: 1
      }
    };
    const handlers = renderSection({ delivery: delivery({ sinks: [consented] }) });
    openSink();
    expect(mode("prefix").checked).toBe(true);
    expect(screen.getByTestId("notification-delivery-content-plaintext").textContent).toContain("Resend");

    const count = screen.getByTestId("notification-delivery-content-prefix-count") as HTMLInputElement;
    fireEvent.change(count, { target: { value: "9999" } });
    fireEvent.blur(count);
    expect(lastSink(handlers).content).toEqual({ kind: "prefix", graphemes: 500 });

    fireEvent.click(mode("full"));
    // Between text modes the existing consent carries over without a second dialog.
    expect(screen.queryByTestId("notification-delivery-consent-accept")).toBeNull();
    expect(lastSink(handlers).content).toEqual({ kind: "full" });
    expect(lastSink(handlers).content_consent).toEqual(consented.content_consent);

    fireEvent.click(mode("namesOnly"));
    expect(lastSink(handlers)).toMatchObject({ content: { kind: "namesOnly" }, content_consent: null });
  });

  test("offers no text mode to a ServerChan destination until a SendKey is saved", () => {
    renderSection({ delivery: delivery({ sinks: [serverChanSink] }) });
    openSink(SC_SINK_ID);
    expect(mode("prefix").disabled).toBe(true);
    expect(mode("full").disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-content-needs-key")).toBeTruthy();
    expect(mode("full").parentElement?.textContent).toContain("1000");
  });
});

describe("NotificationDeliverySection timing", () => {
  test("labels the seconds, bounds them at the Rust maximum, and offers only-when-inactive", () => {
    const handlers = renderSection();
    openSink();
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    expect(dwell.getAttribute("aria-label")).toContain("seconds");
    expect(dwell.value).toBe("300");
    fireEvent.change(dwell, { target: { value: "999999" } });
    fireEvent.blur(dwell);
    expect(lastSink(handlers).timing.dwell_secs).toBe(86_400);

    const interval = screen.getByTestId("notification-delivery-interval") as HTMLInputElement;
    expect(interval.getAttribute("aria-label")).toContain("seconds");
    expect(interval.value).toBe("900");

    fireEvent.click(screen.getByTestId("notification-delivery-only-unfocused"));
    expect(lastSink(handlers).timing.only_when_unfocused).toBe(false);
  });

  test("shows the bound the settings now hold instead of the entry Rust rejected", () => {
    const handlers = actions();
    const { rerender } = render(section({ handlers }));
    openSink();
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    fireEvent.change(dwell, { target: { value: "120" } });
    fireEvent.blur(dwell);
    expect(lastSink(handlers).timing.dwell_secs).toBe(120);
    // Rust kept its own value; the field shows it rather than the typed entry.
    rerender(section({ handlers, delivery: delivery({ sinks: [{ ...resendSink, timing: { ...resendSink.timing, dwell_secs: 60 } }] }) }));
    expect((screen.getByTestId("notification-delivery-dwell") as HTMLInputElement).value).toBe("60");
  });
});

describe("NotificationDeliverySection outcome", () => {
  test("reports setup and says the service accepted, never that the message arrived", () => {
    renderSection({
      runtime: state({ sinks: [sinkState({ last_outcome: { kind: "accepted", at_ms: 1_700_000_000_000 } })] })
    });
    openSink();
    expect(screen.getByTestId("notification-delivery-sink-setup").getAttribute("data-setup")).toBe(
      "configured"
    );
    const outcome = screen.getByTestId("notification-delivery-sink-outcome");
    expect(outcome.getAttribute("data-outcome")).toBe("accepted");
    expect(outcome.textContent).toContain("Last attempt");
    expect(outcome.textContent).toContain("Accepted by the service.");
    expect(screen.getByTestId("notification-delivery-outcome").textContent?.toLowerCase()).not.toContain(
      "delivered"
    );
  });

  test("names the wait when the service rate limited the send", () => {
    renderSection({
      runtime: state({
        sinks: [sinkState({ last_outcome: { kind: "rateLimited", at_ms: 1, retry_after_secs: 120 } })]
      })
    });
    openSink();
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain("120 seconds");
  });

  test("sends one test message through the typed command, and only with a credential", async () => {
    const handlers = actions();
    const { rerender } = render(section({ handlers, runtime: state({ sinks: [sinkState({ secret_configured: false })] }) }));
    openSink();
    expect((screen.getByTestId("notification-delivery-sink-test") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-sink-test-needs-secret")).toBeTruthy();

    rerender(section({ handlers, runtime: state({ sinks: [sinkState()] }) }));
    await act(async () => {
      fireEvent.click(screen.getByTestId("notification-delivery-sink-test"));
    });
    expect(handlers.test).toHaveBeenCalledWith(SINK_ID);
    expect(handlers.test).toHaveBeenCalledTimes(1);
  });

  test("removes a destination and releases the credential it was keyed by", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-sink-remove"));
    expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID);
    await waitFor(() => expect(lastUpdate(handlers).sinks).toEqual([]));
  });

  test("keeps a destination whose credential could not be cleared", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    vi.mocked(handlers.clearSecret).mockRejectedValueOnce(new Error("store unavailable"));
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-sink-remove"));
    await waitFor(() => expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID));
    await act(async () => {});
    // Removing the sink now would orphan a credential no settings name any more.
    expect(handlers.update).not.toHaveBeenCalled();
  });

  test("ships the detail in Japanese", () => {
    setActiveLocaleProfile("ja", "none");
    renderSection({ runtime: state({ sinks: [sinkState({ last_outcome: { kind: "transport", at_ms: 1 } })] }) });
    expect(screen.getByRole("heading", { name: "この端末から送信" })).toBeTruthy();
    openSink();
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain("接続できませんでした");
  });
});

describe("NotificationDeliverySection keyword editor (#1427)", () => {
  test("renders the account's rules in the server's own order", () => {
    renderKeywordSection();
    openSink();
    const rows = screen.getAllByTestId("notification-keyword-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText("Alpha")).toBeTruthy();
    expect(within(rows[1]).getByText("Beta")).toBeTruthy();
    // The whole list is the projection, not a locally sorted or reversed copy.
    expect(rows.map((row) => row.querySelector("span")?.textContent)).toEqual(["Alpha", "Beta"]);
    expect(screen.queryByTestId("notification-keyword-empty")).toBeNull();
  });

  test("keeps a rule another client switched off, in order, and says it is off", () => {
    const { keywordHandlers } = renderKeywordSection();
    openSink();
    const rows = screen.getAllByTestId("notification-keyword-row");
    // The disabled rule is still the second row, in the server's own order.
    expect(within(rows[1]).getByText("Beta")).toBeTruthy();
    expect(within(rows[1]).getByTestId("notification-keyword-disabled").textContent).toBe(
      "Off on your account"
    );
    // An enabled rule carries no off copy.
    expect(within(rows[0]).queryByTestId("notification-keyword-disabled")).toBeNull();
    // The remove control is unchanged, so a stale off rule can be cleared.
    fireEvent.click(within(rows[1]).getByTestId("notification-keyword-remove"));
    expect(keywordHandlers.remove).toHaveBeenCalledWith(KEYWORD_RULES[1].rule_id);
  });

  test("removes exactly the rule its row names, without patching the delivery block", () => {
    const { handlers, keywordHandlers } = renderKeywordSection();
    openSink();
    const rows = screen.getAllByTestId("notification-keyword-row");
    fireEvent.click(within(rows[1]).getByTestId("notification-keyword-remove"));
    expect(keywordHandlers.remove).toHaveBeenCalledWith(KEYWORD_RULES[1].rule_id);
    expect(keywordHandlers.remove).toHaveBeenCalledTimes(1);
    // Removal is account state; it never rewrites this device's delivery patch.
    expect(handlers.update).not.toHaveBeenCalled();
    expect(keywordHandlers.add).not.toHaveBeenCalled();
  });

  test("adds the trimmed pattern and stays unavailable until the field has text", () => {
    const { keywordHandlers } = renderKeywordSection();
    openSink();
    const input = screen.getByTestId("notification-keyword-input") as HTMLInputElement;
    const add = screen.getByTestId("notification-keyword-add") as HTMLButtonElement;
    expect(add.disabled).toBe(true);

    fireEvent.change(input, { target: { value: "   " } });
    expect(add.disabled).toBe(true);
    // A form submit while empty dispatches nothing.
    fireEvent.click(add);
    expect(keywordHandlers.add).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "  cake  " } });
    expect(add.disabled).toBe(false);
    fireEvent.click(add);
    expect(keywordHandlers.add).toHaveBeenCalledWith("cake");
  });

  test("a refused duplicate leaves the list as it was and says why", () => {
    renderKeywordSection({
      keywordOperation: {
        kind: "failed",
        request_id: 4,
        operation: { kind: "addNotificationKeyword", pattern: "alpha" },
        failureKind: "duplicate"
      }
    });
    openSink();
    // The projection is unchanged: the refused pattern is not shown as a rule.
    const rows = screen.getAllByTestId("notification-keyword-row");
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.querySelector("span")?.textContent)).toEqual(["Alpha", "Beta"]);
    expect(screen.getByTestId("notification-keyword-error").textContent).toBe(
      "That keyword is already in the list."
    );
  });

  test("names the cap and a rule another client already removed", () => {
    const { rerender } = render(
      section({
        keywordOperation: {
          kind: "failed",
          request_id: 6,
          operation: { kind: "addNotificationKeyword", pattern: "cake" },
          failureKind: "limitReached"
        }
      })
    );
    openSink();
    expect(screen.getByTestId("notification-keyword-error").textContent).toBe(
      "Your keyword list is full. Remove one to add another."
    );

    rerender(
      section({
        keywordOperation: {
          kind: "failed",
          request_id: 7,
          operation: {
            kind: "removeNotificationKeyword",
            ruleId: KEYWORD_RULES[0].rule_id
          },
          failureKind: "unknownRule"
        }
      })
    );
    expect(screen.getByTestId("notification-keyword-error").textContent).toBe(
      "That keyword is no longer on the server. Reopen this page and try again."
    );
  });

  test("renders the empty state when the account has no keywords", () => {
    renderKeywordSection({ keywordRules: [] });
    openSink();
    expect(screen.getByTestId("notification-keyword-empty").textContent).toBe(
      "No keywords yet."
    );
    expect(screen.queryByTestId("notification-keyword-list")).toBeNull();
  });

  test("the delivery switch delivers for keywords but adds and removes nothing", () => {
    const { handlers, keywordHandlers } = renderKeywordSection();
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-keywords"));
    expect(lastSink(handlers).targets.keywords).toBe(false);
    expect(keywordHandlers.add).not.toHaveBeenCalled();
    expect(keywordHandlers.remove).not.toHaveBeenCalled();
  });

  test("clears the field only once Rust confirms the add", () => {
    const { rerender } = render(section());
    openSink();
    const input = screen.getByTestId("notification-keyword-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "cake" } });
    expect(input.value).toBe("cake");

    // Rust's confirmation is a succeeded add; the acknowledged value is empty.
    rerender(
      section({
        keywordRules: [
          ...KEYWORD_RULES,
          {
            rule_id: "org.matrix.desktop.notify.keyword.cccccccccccccccc",
            pattern: "cake",
            enabled: true
          }
        ],
        keywordOperation: {
          kind: "succeeded",
          request_id: 5,
          operation: { kind: "addNotificationKeyword", pattern: "cake" }
        }
      })
    );
    expect(input.value).toBe("");
    expect(screen.getAllByTestId("notification-keyword-row")).toHaveLength(3);
  });

  test("does not render the editor for a surface with no account projection", () => {
    render(
      <NotificationDeliverySection
        delivery={delivery()}
        state={state()}
        rooms={rooms()}
        spaces={spaces()}
        actions={actions()}
      />
    );
    openSink();
    expect(screen.queryByTestId("notification-keywords")).toBeNull();
    // The switch remains: it is device state and exists without an account.
    expect(screen.getByTestId("notification-delivery-keywords")).toBeTruthy();
  });
});
