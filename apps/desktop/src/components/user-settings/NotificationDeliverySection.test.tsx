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
  NotificationDeliveryPreviewSample,
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
  direct_partner_label: "localAlias",
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
    save: vi.fn(async () => undefined),
    clearSecret: vi.fn(async () => undefined),
    test: vi.fn(async () => undefined),
    // Stands in for Rust's digest renderer: echoes which partner name and mode it was asked for.
    preview: vi.fn(async (sink: NotificationSinkSettings, sample: NotificationDeliveryPreviewSample) => {
      const partner = sink.direct_partner_label === "localAlias" ? sample.partnerAlias : sample.partnerUserId;
      return {
        sender_name: sink.config.kind === "serverChan" ? "" : sink.sender_name || "Koushi",
        subject: `Direct message from ${partner}`,
        body: sink.content.kind === "namesOnly" ? `Direct message from ${partner}` : sample.messageText
      };
    })
  };
}

// Synthetic values only; none of them is a real credential.
const SYNTHETIC_API_KEY = "synthetic-api-key-0000";
const SYNTHETIC_SENDKEY = "SCTsynthetic123";

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

type SaveCall = { sink: NotificationSinkSettings; secret: string | null; acceptContentConsent: boolean };

function saveCalls(handlers: NotificationDeliveryActions): SaveCall[] {
  return vi
    .mocked(handlers.save)
    .mock.calls.map(([sink, secret, acceptContentConsent]) => ({ sink, secret, acceptContentConsent }));
}

function lastSave(handlers: NotificationDeliveryActions): SaveCall {
  const calls = saveCalls(handlers);
  if (calls.length === 0) {
    throw new Error("Save was never dispatched");
  }
  return calls[calls.length - 1];
}

function lastUpdate(handlers: NotificationDeliveryActions): NotificationDeliverySettings {
  const calls = vi.mocked(handlers.update).mock.calls;
  return calls[calls.length - 1][0];
}

async function clickSave() {
  await act(async () => {
    fireEvent.click(screen.getByTestId("notification-delivery-save"));
  });
}

function saveButton(): HTMLButtonElement {
  return screen.getByTestId("notification-delivery-save") as HTMLButtonElement;
}

function saveStatus(): string {
  return screen.getByTestId("notification-delivery-save-status").textContent ?? "";
}

function typeSecret(value: string) {
  fireEvent.change(screen.getByTestId("notification-delivery-sink-secret"), { target: { value } });
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

function pickKind(kind: "resend" | "smtp" | "serverChan") {
  fireEvent.click(screen.getByTestId("notification-delivery-add"));
  const option = screen
    .getAllByTestId("notification-delivery-pick-kind")
    .find((candidate) => candidate.getAttribute("data-kind") === kind);
  if (!option) {
    throw new Error(`no ${kind} option`);
  }
  fireEvent.click(option);
}

function mode(name: string): HTMLInputElement {
  const radio = screen
    .getAllByTestId("notification-delivery-content-mode")
    .find((candidate) => candidate.getAttribute("data-mode") === name);
  if (!radio) {
    throw new Error(`no ${name} mode`);
  }
  return radio as HTMLInputElement;
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

  test("summarises targets, content and timing on each row", () => {
    renderSection({
      delivery: delivery({
        sinks: [
          {
            ...resendSink,
            targets: {
              direct_messages: true,
              mentions: false,
              keywords: true,
              watched: [{ kind: "room", room_id: "!alpha:example.invalid" }]
            },
            timing: { ...resendSink.timing, dwell_secs: 600 }
          }
        ]
      })
    });
    const chips = within(screen.getByTestId("notification-delivery-sink-chips"));
    const text = (key: string) =>
      chips.getAllByText(/./).find((chip) => chip.getAttribute("data-chip") === key)?.textContent;
    expect(text("targets")).toBe("DMs · Keywords · 1 watched");
    expect(text("content")).toBe("Names only");
    expect(text("timing")).toBe("After 10 min");
  });

  test("marks a destination whose last send failed", () => {
    renderSection({
      runtime: state({ sinks: [sinkState({ last_outcome: { kind: "invalidSecret", at_ms: 1 } })] })
    });
    const row = screen.getByTestId("notification-delivery-sink");
    expect(row.getAttribute("data-state")).toBe("failed");
    expect(within(row).getByTestId("notification-delivery-sink-failed")).toBeTruthy();
  });

  test("opens a destination's detail in a dialog over the list and closes it with Cancel", () => {
    renderSection();
    openSink();
    const detail = screen.getByTestId("notification-delivery-detail");
    expect(detail.getAttribute("data-draft")).toBe("false");
    // #1488: the destination being edited is the foreground surface, titled by its service.
    const dialog = detail.closest("dialog");
    expect(dialog?.getAttribute("aria-label")).toBe("Resend (email API)");
    expect(within(dialog!).getByTestId("notification-delivery-footer")).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-sink")).toBeTruthy();
    for (const part of ["credentials", "targets", "content", "timing", "outcome", "footer"]) {
      expect(within(detail).getByTestId(`notification-delivery-${part}`)).toBeTruthy();
    }
    fireEvent.click(screen.getByTestId("notification-delivery-cancel"));
    expect(screen.queryByTestId("notification-delivery-detail")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
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
  test("picks a kind, fills the detail, and saves the destination and its key in one command", async () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();

    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    const kinds = screen.getAllByTestId("notification-delivery-pick-kind");
    expect(kinds.map((kind) => kind.getAttribute("data-kind"))).toEqual(["resend", "smtp", "serverChan"]);
    fireEvent.click(kinds[0]);

    const detail = screen.getByTestId("notification-delivery-detail");
    expect(detail.getAttribute("data-draft")).toBe("true");
    expect(detail.closest("dialog")?.getAttribute("aria-label")).toBe("New destination: Resend (email API)");
    // The recipient field says it takes an email address, and how Resend reaches it.
    expect(screen.getByText("Recipient email address")).toBeTruthy();
    expect((screen.getByTestId("notification-delivery-sink-to") as HTMLInputElement).placeholder).toBe(
      "you@example.com"
    );
    expect(screen.getByTestId("notification-delivery-resend-to-hint").textContent).toContain("Resend account");
    // A new destination takes its key in the same screen; content waits for nothing.
    expect(screen.getByTestId("notification-delivery-sink-secret")).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-content")).toBeTruthy();
    // Resend's sender is optional and starts as the shared one, under Advanced.
    const advanced = screen.getByTestId("notification-delivery-advanced");
    const from = within(advanced).getByTestId("notification-delivery-sink-from") as HTMLInputElement;
    expect(from.value).toBe("onboarding@resend.dev");
    const name = within(advanced).getByTestId("notification-delivery-sink-name") as HTMLInputElement;
    expect(name.value).toBe("Koushi");
    expect(name.placeholder).toBe("Koushi");
    expect(name.getAttribute("aria-required")).toBeNull();
    expect(within(advanced).getByTestId("notification-delivery-resend-hint").textContent).toContain(
      "resend.com/domains"
    );

    fireEvent.change(screen.getByTestId("notification-delivery-sink-to"), {
      target: { value: " recipient@example.invalid " }
    });
    expect(handlers.save).not.toHaveBeenCalled();
    expect(handlers.update).not.toHaveBeenCalled();

    typeSecret(SYNTHETIC_API_KEY);
    await clickSave();

    expect(handlers.save).toHaveBeenCalledTimes(1);
    const { sink, secret, acceptContentConsent } = lastSave(handlers);
    expect(secret).toBe(SYNTHETIC_API_KEY);
    expect(acceptContentConsent).toBe(false);
    expect(sink.id).toMatch(/^sink-[a-z0-9-]+$/);
    expect(sink).toMatchObject({
      destination: "recipient@example.invalid",
      from_address: "onboarding@resend.dev",
      sender_name: "Koushi",
      secret_binding: null,
      targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
      content: { kind: "namesOnly" },
      content_consent: null,
      timing: { dwell_secs: 300, min_interval_secs: 900, only_when_unfocused: true },
      config: { kind: "resend", endpoint: "https://api.resend.com/emails" }
    });
    expect(handlers.update).not.toHaveBeenCalled();
    // The key left once, and the page never shows it.
    expect(document.body.innerHTML).not.toContain(SYNTHETIC_API_KEY);
    expect(document.body.textContent).not.toContain(SYNTHETIC_API_KEY);
    // A settled save ends the edit: the dialog closes back to the list.
    expect(screen.queryByTestId("notification-delivery-detail")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  test("a new destination needs its key before Save dispatches", async () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    pickKind("resend");
    fireEvent.change(screen.getByTestId("notification-delivery-sink-to"), {
      target: { value: "recipient@example.invalid" }
    });
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();
    expect(saveStatus()).toContain("Enter the Resend API key");
  });

  test("names the missing fields instead of saving an incomplete destination", async () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    pickKind("smtp");
    typeSecret(SYNTHETIC_API_KEY);
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();
    expect(saveStatus()).toContain("required fields");
  });

  test("an SMTP destination keeps From required and the sender name optional", async () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    pickKind("smtp");
    expect(screen.queryByTestId("notification-delivery-advanced")).toBeNull();
    const name = screen.getByTestId("notification-delivery-sink-name") as HTMLInputElement;
    expect(name.value).toBe("Koushi");
    expect(name.getAttribute("aria-required")).toBeNull();
    expect(screen.getByTestId("notification-delivery-sink-from").getAttribute("aria-required")).toBe("true");

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
    typeSecret(SYNTHETIC_API_KEY);
    await clickSave();
    expect(lastSave(handlers).sink).toMatchObject({
      from_address: "sender@example.invalid",
      config: { kind: "smtp", preset: "gmail", host: "smtp.example.invalid", port: 587, tls: "startTls" }
    });
  });

  test("cancelling the picker or discarding a new destination adds nothing", () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    fireEvent.click(screen.getByTestId("notification-delivery-add"));
    fireEvent.click(screen.getByTestId("notification-delivery-pick-cancel"));
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();

    expect(screen.queryByRole("dialog")).toBeNull();

    pickKind("resend");
    fireEvent.click(screen.getByTestId("notification-delivery-cancel"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(handlers.save).not.toHaveBeenCalled();
    expect(handlers.update).not.toHaveBeenCalled();
    expect(screen.getByTestId("notification-delivery-sinks-empty")).toBeTruthy();
  });

  test("a ServerChan destination explains the setup and saves with its SendKey", async () => {
    const handlers = renderSection({ delivery: delivery({ sinks: [] }) });
    pickKind("serverChan");

    const steps = screen.getByTestId("notification-delivery-serverchan-steps");
    expect(steps.querySelectorAll("li")).toHaveLength(3);
    expect(steps.textContent).toContain("SendKey");
    expect(steps.textContent).toContain("SCT");
    expect(screen.getByTestId("notification-delivery-serverchan-link").textContent).toContain(
      "sct.ftqq.com"
    );
    expect(screen.getByTestId("notification-delivery-serverchan-limits")).toBeTruthy();
    expect(screen.queryByTestId("notification-delivery-sink-from")).toBeNull();
    expect(screen.queryByTestId("notification-delivery-sink-name")).toBeNull();

    typeSecret(` ${SYNTHETIC_SENDKEY} `);
    await clickSave();
    expect(lastSave(handlers).secret).toBe(SYNTHETIC_SENDKEY);
    expect(lastSave(handlers).sink.config).toEqual({
      kind: "serverChan",
      endpoint: "https://sctapi.ftqq.com"
    });
  });

  test("shows a refused save and keeps the draft", async () => {
    const handlers = actions();
    vi.mocked(handlers.save).mockRejectedValueOnce(new Error("refused"));
    renderSection({ delivery: delivery({ sinks: [] }), handlers });
    pickKind("resend");
    fireEvent.change(screen.getByTestId("notification-delivery-sink-to"), {
      target: { value: "recipient@example.invalid" }
    });
    typeSecret(SYNTHETIC_API_KEY);
    await clickSave();
    expect(handlers.save).toHaveBeenCalledTimes(1);
    expect(saveStatus()).toContain("Could not save");
    expect(screen.getByTestId("notification-delivery-detail").getAttribute("data-draft")).toBe("true");
    expect((screen.getByTestId("notification-delivery-sink-to") as HTMLInputElement).value).toBe(
      "recipient@example.invalid"
    );
    // A refused save keeps the dialog open on the draft.
    // The key was taken out of the field before the command left, so a refusal leaves none.
    expect((screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement).value).toBe("");
    expect(document.body.innerHTML).not.toContain(SYNTHETIC_API_KEY);
  });

  test("says it is saving while the command is in flight", async () => {
    const handlers = actions();
    let settle: () => void = () => undefined;
    vi.mocked(handlers.save).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          settle = resolve;
        })
    );
    renderSection({ runtime: state({ sinks: [sinkState()] }), handlers });
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-direct-messages"));
    await clickSave();
    expect(saveButton().disabled).toBe(true);
    expect(saveButton().textContent).toBe("Saving…");
    fireEvent.click(saveButton());
    expect(handlers.save).toHaveBeenCalledTimes(1);
    // The dialog cannot be closed or cancelled under an in-flight save.
    const dialog = screen.getByTestId("notification-delivery-detail").closest("dialog")!;
    expect((dialog.querySelector(".app-modal-header button") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("notification-delivery-cancel") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => settle());
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("NotificationDeliverySection drafts", () => {
  test("edits stay local until Save, and Save sends them once with no new key", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    expect(saveButton().disabled).toBe(true);
    fireEvent.click(screen.getByTestId("notification-delivery-direct-messages"));
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    fireEvent.change(dwell, { target: { value: "120" } });
    fireEvent.blur(dwell);
    expect(handlers.save).not.toHaveBeenCalled();
    expect(handlers.update).not.toHaveBeenCalled();
    expect(saveStatus()).toBe("Unsaved changes");

    await clickSave();
    expect(handlers.save).toHaveBeenCalledTimes(1);
    const { sink, secret, acceptContentConsent } = lastSave(handlers);
    expect(secret).toBeNull();
    expect(acceptContentConsent).toBe(false);
    expect(sink.targets.direct_messages).toBe(false);
    expect(sink.timing.dwell_secs).toBe(120);
    expect(handlers.update).not.toHaveBeenCalled();
  });

  test("Cancel drops the draft and its typed key; reopening shows Rust's copy", () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    const to = screen.getByTestId("notification-delivery-sink-to") as HTMLInputElement;
    fireEvent.change(to, { target: { value: "elsewhere@example.invalid" } });
    fireEvent.click(screen.getByTestId("notification-delivery-mentions"));
    typeSecret(SYNTHETIC_API_KEY);
    fireEvent.click(screen.getByTestId("notification-delivery-cancel"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.body.innerHTML).not.toContain(SYNTHETIC_API_KEY);

    openSink();
    expect(handlers.save).not.toHaveBeenCalled();
    expect((screen.getByTestId("notification-delivery-sink-to") as HTMLInputElement).value).toBe(
      "recipient@example.invalid"
    );
    expect(screen.getByTestId("notification-delivery-mentions").getAttribute("aria-checked")).toBe("true");
    expect((screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement).value).toBe("");
    expect(saveButton().disabled).toBe(true);
  });

  test("shows the value the settings hold once the save settles", async () => {
    const handlers = actions();
    const { rerender } = render(section({ handlers }));
    openSink();
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    fireEvent.change(dwell, { target: { value: "120" } });
    fireEvent.blur(dwell);
    await clickSave();
    expect(lastSave(handlers).sink.timing.dwell_secs).toBe(120);
    // Rust kept its own value; reopened, the field shows it rather than the typed entry.
    rerender(
      section({
        handlers,
        delivery: delivery({ sinks: [{ ...resendSink, timing: { ...resendSink.timing, dwell_secs: 60 } }] })
      })
    );
    openSink();
    expect((screen.getByTestId("notification-delivery-dwell") as HTMLInputElement).value).toBe("60");
  });
});

describe("NotificationDeliverySection credentials", () => {
  test("keeps a Resend API key write-only and sends it only on Save", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    const secret = screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement;
    expect(secret.getAttribute("aria-label")).toBe("Resend API key");
    expect(secret.getAttribute("autocomplete")).toBe("off");
    expect(secret.value).toBe("");
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain("Saved");

    typeSecret(SYNTHETIC_API_KEY);
    expect(handlers.save).not.toHaveBeenCalled();
    await clickSave();
    expect(lastSave(handlers).secret).toBe(SYNTHETIC_API_KEY);
    await waitFor(() => expect(secret.value).toBe(""));
    // The value never reaches the rendered page.
    expect(document.body.innerHTML).not.toContain(SYNTHETIC_API_KEY);
    // There is no separate clear button; removing the destination is what releases the key.
    expect(screen.queryByTestId("notification-delivery-sink-clear-secret")).toBeNull();
  });

  test("refuses a ServerChan3 sctp key before it leaves and accepts an SCT SendKey", async () => {
    const handlers = renderSection({
      delivery: delivery({ sinks: [serverChanSink] }),
      runtime: state({ sinks: [sinkState({ sink_id: SC_SINK_ID, secret_configured: false })] })
    });
    openSink(SC_SINK_ID);
    const secret = screen.getByTestId("notification-delivery-sink-secret") as HTMLInputElement;
    expect(secret.getAttribute("aria-label")).toBe("SendKey");

    typeSecret("sctp123tSYNTHETIC");
    const problem = screen.getByTestId("notification-delivery-sink-secret-problem");
    expect(problem.textContent).toContain("sctp");
    expect(problem.textContent).not.toContain("SYNTHETIC");
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();

    typeSecret(` ${SYNTHETIC_SENDKEY} `);
    expect(screen.queryByTestId("notification-delivery-sink-secret-problem")).toBeNull();
    await clickSave();
    expect(handlers.save).toHaveBeenCalledTimes(1);
    expect(lastSave(handlers).secret).toBe(SYNTHETIC_SENDKEY);
  });

  test("asks for the password again when the server or account changes", async () => {
    const boundSink = { ...smtpSink, secret_binding: "smtp:smtp.example.invalid:465:sender@example.invalid" };
    const handlers = renderSection({
      delivery: delivery({ sinks: [boundSink] }),
      runtime: state({ sinks: [sinkState()] })
    });
    openSink();
    const host = screen.getByTestId("notification-delivery-sink-host") as HTMLInputElement;
    fireEvent.change(host, { target: { value: "relay.example.invalid" } });
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain(
      "server or account changed"
    );
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();
    expect(saveStatus()).toContain("server or account changed");

    typeSecret(SYNTHETIC_API_KEY);
    await clickSave();
    const { sink, secret } = lastSave(handlers);
    expect(secret).toBe(SYNTHETIC_API_KEY);
    expect(sink.config).toMatchObject({ host: "relay.example.invalid", port: 465 });
    // The binding goes back to Rust untouched; Rust is what decides the credential is stale.
    expect(sink.secret_binding).toBe(boundSink.secret_binding);
  });

  test("says why a stored password is gone when Rust retired it", () => {
    renderSection({
      delivery: delivery({ sinks: [smtpSink] }),
      runtime: state({ sinks: [sinkState({ secret_configured: false, secret_retired: true })] })
    });
    openSink();
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

  test("keeps the stored key for a destination, name or from address change", async () => {
    const boundSink = { ...resendSink, secret_binding: "resend:https://api.example.invalid/emails" };
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
      fireEvent.change(screen.getByTestId(testId), { target: { value } });
    }
    expect(screen.getByTestId("notification-delivery-sink-secret-state").textContent).toContain("Saved");
    await clickSave();
    expect(handlers.clearSecret).not.toHaveBeenCalled();
    const { sink, secret } = lastSave(handlers);
    expect(secret).toBeNull();
    expect(sink.secret_binding).toBe(boundSink.secret_binding);
    expect(sink).toMatchObject({
      from_address: "other@example.invalid",
      sender_name: "Another name",
      destination: "elsewhere@example.invalid"
    });
  });

  test("keeps the endpoint override, sender and name under Advanced", () => {
    renderSection();
    openSink();
    const advanced = screen.getByTestId("notification-delivery-advanced");
    expect(advanced.tagName).toBe("DETAILS");
    for (const field of ["endpoint", "from", "name"]) {
      expect(within(advanced).getByTestId(`notification-delivery-sink-${field}`)).toBeTruthy();
    }
    expect(within(advanced).getByTestId("notification-delivery-resend-hint").textContent).toContain(
      "onboarding@resend.dev"
    );
  });
});

describe("NotificationDeliverySection targets", () => {
  test("toggles the categories for this destination only", async () => {
    const other = { ...resendSink, id: "sink-other" };
    const handlers = renderSection({ delivery: delivery({ sinks: [resendSink, other] }) });
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-direct-messages"));
    expect(screen.getByTestId("notification-delivery-keywords-hint").textContent).toContain("homeserver");
    await clickSave();
    expect(lastSave(handlers).sink).toMatchObject({ id: SINK_ID, targets: { direct_messages: false } });
  });

  test("names each watched entry from the room list, adds one and removes one", async () => {
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
    expect(screen.queryByTestId("notification-delivery-watched-row")).toBeNull();

    fireEvent.change(screen.getByTestId("notification-delivery-watched-add"), {
      target: { value: "space:!space:example.invalid" }
    });
    await clickSave();
    expect(lastSave(handlers).sink.targets.watched).toEqual([
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
  const consent = {
    sink_kind: "resend" as const,
    endpoint: "https://api.example.invalid/emails",
    recipient: "recipient@example.invalid",
    accepted_at_ms: 1
  };

  test("starts at names only and says the provider receives plaintext only once text is on", () => {
    renderSection();
    openSink();
    expect(mode("namesOnly").checked).toBe(true);
    expect(screen.queryByTestId("notification-delivery-content-plaintext")).toBeNull();
    expect(screen.queryByTestId("notification-delivery-consent")).toBeNull();
    expect(screen.getByTestId("notification-delivery-content-fallback").textContent).toContain(
      "names only"
    );
    expect(screen.getByTestId("notification-delivery-content-independent")).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-content-always-sent").textContent).toContain(
      "unread counts"
    );
    // The full mode states its bound, under the option, rather than promising the whole message.
    fireEvent.click(mode("full"));
    const option = mode("full").closest(".notification-delivery-content-option") as HTMLElement;
    expect(within(option).getByTestId("notification-delivery-content-limit").textContent).toContain("4000");
    expect(within(option).getByTestId("notification-delivery-content-plaintext")).toBeTruthy();
    expect(within(option).getByTestId("notification-delivery-consent")).toBeTruthy();
  });

  test("shows the plaintext warning inline and sends the accepted answer, never a consent", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    fireEvent.click(mode("prefix"));
    expect(mode("prefix").checked).toBe(true);
    // The warning is inline in the editor, not a second dialog on top of it.
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.getByTestId("notification-delivery-consent-body").textContent).toContain(
      "recipient@example.invalid"
    );
    // Unchecked, Save refuses and says why.
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();
    expect(saveStatus()).toContain("Check the box");

    fireEvent.click(screen.getByTestId("notification-delivery-consent-acknowledge"));
    await clickSave();
    const { sink, acceptContentConsent, secret } = lastSave(handlers);
    expect(acceptContentConsent).toBe(true);
    expect(secret).toBeNull();
    expect(sink.content).toEqual({ kind: "prefix", graphemes: 80 });
    // Rust mints the consent; the renderer sends none.
    expect(sink.content_consent).toBeNull();
  });

  test("reads a consent for another recipient as needing a new answer", () => {
    renderSection({
      delivery: delivery({
        sinks: [{ ...resendSink, content: { kind: "full" }, content_consent: { ...consent, recipient: "other@example.invalid" } }]
      })
    });
    openSink();
    expect(screen.getByTestId("notification-delivery-consent")).toBeTruthy();
  });

  test("a matching consent covers both text modes; names only drops it", async () => {
    const consented: NotificationSinkSettings = {
      ...resendSink,
      content: { kind: "prefix", graphemes: 40 },
      content_consent: consent
    };
    const handlers = renderSection({ delivery: delivery({ sinks: [consented] }) });
    openSink();
    expect(mode("prefix").checked).toBe(true);
    expect(screen.getByTestId("notification-delivery-content-plaintext").textContent).toContain("Resend");
    expect(screen.queryByTestId("notification-delivery-consent")).toBeNull();

    const count = screen.getByTestId("notification-delivery-content-prefix-count") as HTMLInputElement;
    fireEvent.change(count, { target: { value: "9999" } });
    fireEvent.blur(count);
    fireEvent.click(mode("full"));
    // Between text modes the existing consent carries over without a second question.
    expect(screen.queryByTestId("notification-delivery-consent")).toBeNull();
    await clickSave();
    expect(lastSave(handlers)).toMatchObject({
      sink: { content: { kind: "full" }, content_consent: consent },
      acceptContentConsent: false
    });

    openSink();
    fireEvent.click(mode("namesOnly"));
    await clickSave();
    expect(lastSave(handlers).sink).toMatchObject({ content: { kind: "namesOnly" }, content_consent: null });
  });

  test("clamps the prefix length to the Rust bound", async () => {
    const handlers = renderSection({
      delivery: delivery({ sinks: [{ ...resendSink, content: { kind: "prefix", graphemes: 40 }, content_consent: consent }] })
    });
    openSink();
    const count = screen.getByTestId("notification-delivery-content-prefix-count") as HTMLInputElement;
    fireEvent.change(count, { target: { value: "9999" } });
    fireEvent.blur(count);
    await clickSave();
    expect(lastSave(handlers).sink.content).toEqual({ kind: "prefix", graphemes: 500 });
  });

  test("offers no text mode to a ServerChan destination until a SendKey is entered", () => {
    renderSection({ delivery: delivery({ sinks: [serverChanSink] }) });
    openSink(SC_SINK_ID);
    expect(mode("prefix").disabled).toBe(true);
    expect(mode("full").disabled).toBe(true);
    expect(screen.getByTestId("notification-delivery-content-needs-key")).toBeTruthy();

    typeSecret(SYNTHETIC_SENDKEY);
    expect(mode("prefix").disabled).toBe(false);
    expect(screen.queryByTestId("notification-delivery-content-needs-key")).toBeNull();
    fireEvent.click(mode("full"));
    expect(screen.getByTestId("notification-delivery-content-limit").textContent).toContain("1000");
  });

  test("a new ServerChan SendKey asks for the plaintext answer again", async () => {
    const bound: NotificationSinkSettings = {
      ...serverChanSink,
      secret_binding: "serverchan:https://sctapi.ftqq.com#0a1b",
      content: { kind: "full" },
      content_consent: {
        sink_kind: "serverChan",
        endpoint: "https://sctapi.ftqq.com",
        recipient: "sendkey#0a1b",
        accepted_at_ms: 1
      }
    };
    const handlers = renderSection({
      delivery: delivery({ sinks: [bound] }),
      runtime: state({ sinks: [sinkState({ sink_id: SC_SINK_ID })] })
    });
    openSink(SC_SINK_ID);
    expect(screen.queryByTestId("notification-delivery-consent")).toBeNull();
    typeSecret(SYNTHETIC_SENDKEY);
    expect(screen.getByTestId("notification-delivery-consent")).toBeTruthy();
    await clickSave();
    expect(handlers.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("notification-delivery-consent-acknowledge"));
    await clickSave();
    expect(lastSave(handlers)).toMatchObject({ secret: SYNTHETIC_SENDKEY, acceptContentConsent: true });
  });
});

describe("NotificationDeliverySection direct message names and preview (#1492)", () => {
  test("names partners by alias by default and saves the unchecked choice", async () => {
    const handlers = renderSection();
    openSink();
    const alias = screen.getByTestId("notification-delivery-partner-alias") as HTMLInputElement;
    expect(alias.checked).toBe(true);
    // The hint says where the alias goes and what replaces it.
    const hint = screen.getByTestId("notification-delivery-partner-alias-hint").textContent ?? "";
    expect(hint).toContain("Resend");
    expect(hint).toContain("@user:server");

    fireEvent.click(alias);
    await clickSave();
    expect(lastSave(handlers).sink.direct_partner_label).toBe("matrixId");
  });

  test("renders Rust's preview of the unsaved sink and follows edits", async () => {
    const handlers = renderSection();
    openSink();
    await waitFor(() =>
      expect(screen.getByTestId("notification-delivery-preview-subject").textContent).toBe(
        "Direct message from Alex (design team)"
      )
    );
    expect(screen.getByTestId("notification-delivery-preview-sender").textContent).toBe("Koushi");

    fireEvent.click(screen.getByTestId("notification-delivery-partner-alias"));
    await waitFor(() =>
      expect(screen.getByTestId("notification-delivery-preview-subject").textContent).toBe(
        "Direct message from @alex:example.org"
      )
    );
    const [sink, sample] = vi.mocked(handlers.preview).mock.calls.at(-1)!;
    expect(sink.direct_partner_label).toBe("matrixId");
    expect(sample.partnerUserId).toBe("@alex:example.org");

    fireEvent.click(mode("full"));
    await waitFor(() =>
      expect(screen.getByTestId("notification-delivery-preview-body").textContent).toBe(
        "Are you free for a quick call this afternoon?"
      )
    );
  });

  test("a ServerChan preview has a title and no sender", async () => {
    renderSection({ delivery: delivery({ sinks: [serverChanSink] }) });
    openSink(SC_SINK_ID);
    const preview = screen.getByTestId("notification-delivery-preview");
    await waitFor(() => expect(screen.getByTestId("notification-delivery-preview-subject")).toBeTruthy());
    expect(screen.queryByTestId("notification-delivery-preview-sender")).toBeNull();
    expect(preview.textContent).toContain("Title");
  });

  test("hides a failed preview instead of showing stale text", async () => {
    const handlers = actions();
    vi.mocked(handlers.preview).mockRejectedValue(new Error("unavailable"));
    renderSection({ handlers });
    openSink();
    await waitFor(() => expect(handlers.preview).toHaveBeenCalled());
    expect(screen.queryByTestId("notification-delivery-preview-subject")).toBeNull();
  });
});

describe("NotificationDeliverySection timing", () => {
  test("labels the seconds, bounds them at the Rust maximum, and offers only-when-inactive", async () => {
    const handlers = renderSection();
    openSink();
    const dwell = screen.getByTestId("notification-delivery-dwell") as HTMLInputElement;
    expect(dwell.getAttribute("aria-label")).toContain("seconds");
    expect(dwell.value).toBe("300");
    fireEvent.change(dwell, { target: { value: "999999" } });
    fireEvent.blur(dwell);

    const interval = screen.getByTestId("notification-delivery-interval") as HTMLInputElement;
    expect(interval.getAttribute("aria-label")).toContain("seconds");
    expect(interval.value).toBe("900");

    fireEvent.click(screen.getByTestId("notification-delivery-only-unfocused"));
    await clickSave();
    expect(lastSave(handlers).sink.timing).toMatchObject({ dwell_secs: 86_400, only_when_unfocused: false });
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

  test("offers no test or remove for a destination that is not saved yet", () => {
    renderSection({ delivery: delivery({ sinks: [] }) });
    pickKind("resend");
    expect(screen.queryByTestId("notification-delivery-outcome")).toBeNull();
    expect(screen.queryByTestId("notification-delivery-sink-delete")).toBeNull();
    expect(screen.queryByTestId("notification-delivery-sink-remove")).toBeNull();
  });

  test("removes a destination and releases the credential it was keyed by", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    openSink();
    // Delete is a dialog-level action beside Cancel and Save, not part of the Status block.
    const footer = screen.getByTestId("notification-delivery-footer");
    const outcome = screen.getByTestId("notification-delivery-outcome");
    expect(within(outcome).queryByTestId("notification-delivery-sink-delete")).toBeNull();
    fireEvent.click(within(footer).getByTestId("notification-delivery-sink-delete"));
    // It asks first, and backing out deletes nothing.
    expect(handlers.clearSecret).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("notification-delivery-sink-delete-keep"));
    expect(screen.queryByTestId("notification-delivery-sink-delete-confirm")).toBeNull();
    expect(handlers.clearSecret).not.toHaveBeenCalled();
    fireEvent.click(within(footer).getByTestId("notification-delivery-sink-delete"));
    expect(screen.getByTestId("notification-delivery-sink-delete-confirm").textContent).toContain(
      "Delete this destination and the credential saved for it?"
    );
    fireEvent.click(screen.getByTestId("notification-delivery-sink-remove"));
    expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID);
    await waitFor(() => expect(lastUpdate(handlers).sinks).toEqual([]));
  });

  test("keeps a destination whose credential could not be cleared", async () => {
    const handlers = renderSection({ runtime: state({ sinks: [sinkState()] }) });
    vi.mocked(handlers.clearSecret).mockRejectedValueOnce(new Error("store unavailable"));
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-sink-delete"));
    fireEvent.click(screen.getByTestId("notification-delivery-sink-remove"));
    await waitFor(() => expect(handlers.clearSecret).toHaveBeenCalledWith(SINK_ID));
    await act(async () => {});
    // Removing the sink now would orphan a credential no settings name any more.
    expect(handlers.update).not.toHaveBeenCalled();
  });

  test("ships the list and the detail in Japanese", () => {
    setActiveLocaleProfile("ja", "none");
    renderSection({ runtime: state({ sinks: [sinkState({ last_outcome: { kind: "transport", at_ms: 1 } })] }) });
    expect(screen.getByRole("heading", { name: "この端末から送信" })).toBeTruthy();
    expect(screen.getByTestId("notification-delivery-sink-status").textContent).not.toBe("Configured");
    openSink();
    expect(screen.getByTestId("notification-delivery-sink-outcome").textContent).toContain("接続できませんでした");
    expect(saveButton().textContent).toBe("保存");
    expect(screen.getByTestId("notification-delivery-cancel").textContent).toBe("キャンセル");
    expect(screen.getByTestId("notification-delivery-detail").closest("dialog")?.getAttribute("aria-label")).toBe(
      "Resend（メールAPI）"
    );
    expect(screen.getByText("宛先メールアドレス")).toBeTruthy();
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

  test("the delivery switch delivers for keywords but adds and removes nothing", async () => {
    const { handlers, keywordHandlers } = renderKeywordSection();
    openSink();
    fireEvent.click(screen.getByTestId("notification-delivery-keywords"));
    await clickSave();
    expect(lastSave(handlers).sink.targets.keywords).toBe(false);
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
