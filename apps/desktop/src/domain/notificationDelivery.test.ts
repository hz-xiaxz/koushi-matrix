import { afterEach, describe, expect, test } from "vitest";

import {
  DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS,
  DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS,
  DEFAULT_NOTIFICATION_SENDER_NAME,
  DEFAULT_RESEND_FROM_ADDRESS,
  MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES,
  MAX_NOTIFICATION_DELIVERY_SECONDS,
  clampNotificationDeliveryPrefixGraphemes,
  clampNotificationDeliverySeconds,
  newNotificationSink,
  notificationConsentForCurrent,
  notificationConsentRecipient,
  notificationContentConsented,
  notificationDeliveryOutcomeMessage,
  notificationDeliveryServiceLabel,
  notificationDeliverySinkFailed,
  notificationEffectiveContent,
  notificationSinkEndpoint,
  notificationSinkFieldsReady,
  notificationSinkIdFromToken,
  notificationSinkSetup,
  notificationSmtpConfigFromPreset,
  notificationTargetsForLabel,
  notificationTextBudget,
  serverChanSendKeyProblem
} from "./notificationDelivery";
import { setActiveLocaleProfile } from "../i18n/messages";
import type {
  NotificationDeliveryPresetState,
  NotificationDeliverySinkState,
  NotificationSinkConfig,
  NotificationSinkSettings,
  NotificationDeliveryTarget,
  RoomSummary,
  SpaceSummary
} from "./types";

afterEach(() => setActiveLocaleProfile("en", "none"));

const resendSink: NotificationSinkSettings = {
  ...newNotificationSink("sink-one", "resend"),
  destination: "recipient@example.invalid",
  from_address: "sender@example.invalid",
  config: { kind: "resend", endpoint: "https://api.example.invalid/emails" }
};

const smtpSink: NotificationSinkSettings = {
  ...newNotificationSink("sink-two", "smtp"),
  destination: "recipient@example.invalid",
  from_address: "sender@example.invalid",
  sender_name: "Koushi",
  config: {
    kind: "smtp",
    preset: "qq-mail",
    host: "smtp.example.invalid",
    port: 465,
    tls: "implicit",
    username: "sender@example.invalid"
  }
};

const serverChanSink: NotificationSinkSettings = {
  ...newNotificationSink("sink-three", "serverChan"),
  secret_binding: "serverchan:https://sctapi.ftqq.com#0a1b"
};

function sinkState(overrides: Partial<NotificationDeliverySinkState> = {}): NotificationDeliverySinkState {
  return {
    sink_id: "sink-one",
    secret_configured: true,
    secret_retired: false,
    last_outcome: null,
    ...overrides
  };
}

function room(id: string, displayLabel: string): RoomSummary {
  return { room_id: id, display_label: displayLabel } as RoomSummary;
}

function space(id: string, displayName: string): SpaceSummary {
  return { space_id: id, display_name: displayName } as SpaceSummary;
}

function preset(overrides: Partial<NotificationDeliveryPresetState> = {}): NotificationDeliveryPresetState {
  return {
    id: "gmail",
    label: "Gmail",
    host: "smtp.example.invalid",
    port: 587,
    tls: "startTls",
    username_is_full_address: true,
    secret_hint: "settings.notificationDeliverySecretHint.gmail",
    ...overrides
  };
}

describe("notification delivery bounds", () => {
  test("clamps a dwell or interval to the Rust maximum", () => {
    expect(MAX_NOTIFICATION_DELIVERY_SECONDS).toBe(86_400);
    expect(clampNotificationDeliverySeconds(999_999)).toBe(MAX_NOTIFICATION_DELIVERY_SECONDS);
    expect(clampNotificationDeliverySeconds(900)).toBe(900);
    expect(clampNotificationDeliverySeconds(-5)).toBe(0);
    expect(clampNotificationDeliverySeconds(Number.NaN)).toBe(0);
    expect(clampNotificationDeliverySeconds(12.7)).toBe(12);
  });

  test("clamps the plaintext prefix to the Rust maximum and to at least one grapheme", () => {
    expect(MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES).toBe(500);
    expect(clampNotificationDeliveryPrefixGraphemes(5_000)).toBe(500);
    expect(clampNotificationDeliveryPrefixGraphemes(80)).toBe(80);
    expect(clampNotificationDeliveryPrefixGraphemes(0)).toBe(1);
  });
});

describe("notification sink identity", () => {
  test("builds a sink id from the alphabet the credential store will accept", () => {
    expect(notificationSinkIdFromToken("A1b2-C3d4")).toBe("sink-a1b2-c3d4");
    expect(notificationSinkIdFromToken("9f8e/7d6c 5b4a!")).toBe("sink-9f8e7d6c5b4a");
    expect(notificationSinkIdFromToken("x".repeat(200)).length).toBe(64);
  });
});

describe("notification sink endpoint", () => {
  test("mirrors the non-secret endpoint a content consent binds to", () => {
    expect(notificationSinkEndpoint(resendSink)).toBe("https://api.example.invalid/emails");
    expect(notificationSinkEndpoint(smtpSink)).toBe("smtp.example.invalid:465");
    expect(notificationSinkEndpoint(serverChanSink)).toBe("https://sctapi.ftqq.com");
  });
});

describe("notification delivery outcomes", () => {
  test("translates every typed outcome, including the rate-limit wait", () => {
    expect(notificationDeliveryOutcomeMessage(null)).toBeNull();
    expect(notificationDeliveryOutcomeMessage({ kind: "accepted", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultAccepted"
    );
    expect(notificationDeliveryOutcomeMessage({ kind: "invalidSecret", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultInvalidSecret"
    );
    expect(notificationDeliveryOutcomeMessage({ kind: "notPermitted", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultNotPermitted"
    );
    expect(notificationDeliveryOutcomeMessage({ kind: "rejected", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultRejected"
    );
    expect(notificationDeliveryOutcomeMessage({ kind: "transport", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultTransport"
    );
    expect(
      notificationDeliveryOutcomeMessage({ kind: "rateLimited", at_ms: 1, retry_after_secs: null })
        ?.id
    ).toBe("settings.notificationDeliveryResultRateLimited");
    const waited = notificationDeliveryOutcomeMessage({
      kind: "rateLimited",
      at_ms: 1,
      retry_after_secs: 120
    });
    expect(waited?.id).toBe("settings.notificationDeliveryResultRateLimitedAfter");
    expect(waited?.values).toEqual({ seconds: 120 });
  });

  test("only an accepted outcome leaves a sink looking healthy", () => {
    expect(notificationDeliverySinkFailed(null)).toBe(false);
    expect(notificationDeliverySinkFailed({ kind: "accepted", at_ms: 1 })).toBe(false);
    for (const kind of ["invalidSecret", "notPermitted", "rejected", "transport"] as const) {
      expect(notificationDeliverySinkFailed({ kind, at_ms: 1 })).toBe(true);
    }
    expect(
      notificationDeliverySinkFailed({ kind: "rateLimited", at_ms: 1, retry_after_secs: 60 })
    ).toBe(true);
  });
});

describe("content consent", () => {
  test("binds to the sink kind, endpoint and recipient, and lapses when any changes", () => {
    const consented: NotificationSinkSettings = {
      ...resendSink,
      content: { kind: "full" },
      content_consent: notificationConsentForCurrent(resendSink, 1_700_000_000_000)
    };
    expect(notificationContentConsented(consented)).toBe(true);
    expect(notificationEffectiveContent(consented)).toEqual({ kind: "full" });

    const otherRecipient = { ...consented, destination: "other@example.invalid" };
    expect(notificationContentConsented(otherRecipient)).toBe(false);
    expect(notificationEffectiveContent(otherRecipient)).toEqual({ kind: "namesOnly" });

    const otherEndpoint: NotificationSinkSettings = {
      ...consented,
      config: { kind: "resend", endpoint: "https://elsewhere.example.invalid/x" }
    };
    expect(notificationContentConsented(otherEndpoint)).toBe(false);

    const otherKind: NotificationSinkSettings = {
      ...consented,
      content_consent: { ...consented.content_consent!, sink_kind: "smtp" }
    };
    expect(notificationContentConsented(otherKind)).toBe(false);
    expect(notificationEffectiveContent({ ...resendSink, content: { kind: "full" } })).toEqual({
      kind: "namesOnly"
    });
  });

  test("names a ServerChan recipient by the bound SendKey revision, never the key", () => {
    expect(notificationConsentRecipient(serverChanSink)).toBe("sendkey#0a1b");
    // No bound key, no recipient: text modes cannot be consented to.
    expect(notificationConsentRecipient({ ...serverChanSink, secret_binding: null })).toBe("");
    const consented: NotificationSinkSettings = {
      ...serverChanSink,
      content: { kind: "prefix", graphemes: 80 },
      content_consent: notificationConsentForCurrent(serverChanSink, 1)
    };
    expect(notificationContentConsented(consented)).toBe(true);
    // A new SendKey is a new recipient.
    expect(
      notificationContentConsented({
        ...consented,
        secret_binding: "serverchan:https://sctapi.ftqq.com#ffff"
      })
    ).toBe(false);
  });

  test("applies the provider's text budget", () => {
    expect(notificationTextBudget(resendSink)).toBe(4000);
    expect(notificationTextBudget(serverChanSink)).toBe(1000);
  });
});

describe("ServerChan SendKey", () => {
  test("accepts an SCT SendKey and refuses ServerChan3 and malformed keys", () => {
    expect(serverChanSendKeyProblem("SCT12345abcDEF")).toBeNull();
    expect(serverChanSendKeyProblem("  SCT12345abcDEF  ")).toBeNull();
    expect(serverChanSendKeyProblem("sctp123tABC")).toBe("serverChan3Key");
    expect(serverChanSendKeyProblem("SCTP123")).toBe("serverChan3Key");
    expect(serverChanSendKeyProblem("abc")).toBe("malformed");
    expect(serverChanSendKeyProblem("SCT")).toBe("malformed");
    expect(serverChanSendKeyProblem("SCT12-34")).toBe("malformed");
    expect(serverChanSendKeyProblem(`SCT${"a".repeat(200)}`)).toBe("malformed");
  });
});

describe("SMTP presets", () => {
  test("prefills host, port, TLS and the username shape from the Rust preset", () => {
    expect(notificationSmtpConfigFromPreset(preset(), "sender@example.invalid")).toEqual({
      kind: "smtp",
      preset: "gmail",
      host: "smtp.example.invalid",
      port: 587,
      tls: "startTls",
      username: "sender@example.invalid"
    });
    // A relay whose username is not the full address leaves the username to the user.
    expect(
      notificationSmtpConfigFromPreset(preset({ username_is_full_address: false }), "sender@example.invalid")
        .username
    ).toBe("");
  });

  test("names the service from the vendor's own preset label, never a translation", () => {
    const presets = [preset({ id: "qq-mail", label: "QQ Mail", port: 465, tls: "implicit" })];
    expect(notificationDeliveryServiceLabel(smtpSink, presets)).toBe("QQ Mail");
    expect(notificationDeliveryServiceLabel(resendSink, presets)).toBe("Resend (email API)");
    // A stale preset id is a lookup miss, not a guess at another provider.
    const stale: NotificationSinkSettings = {
      ...smtpSink,
      config: { ...smtpSink.config, preset: "gone" } as NotificationSinkConfig
    };
    expect(notificationDeliveryServiceLabel(stale, presets)).toBe("SMTP relay");
  });
});

describe("new sinks", () => {
  test("starts with the conservative Rust defaults", () => {
    const sink = newNotificationSink("sink-new", "resend");
    expect(sink.targets).toEqual({
      direct_messages: true,
      mentions: true,
      keywords: true,
      watched: []
    });
    expect(sink.content).toEqual({ kind: "namesOnly" });
    expect(sink.content_consent).toBeNull();
    expect(sink.secret_binding).toBeNull();
    // The mail kinds start with the defaults Rust would fill in; ServerChan has no sender.
    expect(sink.from_address).toBe(DEFAULT_RESEND_FROM_ADDRESS);
    expect(sink.sender_name).toBe(DEFAULT_NOTIFICATION_SENDER_NAME);
    expect(newNotificationSink("sink-smtp", "smtp").from_address).toBe("");
    expect(newNotificationSink("sink-smtp", "smtp").sender_name).toBe(DEFAULT_NOTIFICATION_SENDER_NAME);
    expect(newNotificationSink("sink-sc", "serverChan").sender_name).toBe("");
    expect(sink.timing).toEqual({
      dwell_secs: DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS,
      min_interval_secs: DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS,
      only_when_unfocused: true
    });
    expect(DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS).toBe(300);
    expect(DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS).toBe(900);
    expect(newNotificationSink("sink-sc", "serverChan").config).toEqual({
      kind: "serverChan",
      endpoint: "https://sctapi.ftqq.com"
    });
  });

  test("only commits a sink Rust would keep", () => {
    expect(notificationSinkFieldsReady(resendSink)).toBe(true);
    expect(notificationSinkFieldsReady({ ...resendSink, destination: "  " })).toBe(false);
    // A Resend sender is optional (Rust fills in DEFAULT_RESEND_FROM_ADDRESS); SMTP's is not.
    expect(notificationSinkFieldsReady({ ...resendSink, from_address: "" })).toBe(true);
    expect(notificationSinkFieldsReady({ ...smtpSink, from_address: " " })).toBe(false);
    expect(notificationSinkFieldsReady(smtpSink)).toBe(true);
    expect(
      notificationSinkFieldsReady({ ...smtpSink, config: { ...smtpSink.config, host: "" } as NotificationSinkConfig })
    ).toBe(false);
    // ServerChan has no non-secret fields: the SendKey names the recipient.
    expect(notificationSinkFieldsReady(newNotificationSink("sink-sc", "serverChan"))).toBe(true);
  });

  test("reports setup completeness, never delivery", () => {
    expect(notificationSinkSetup({ ...resendSink, destination: "" }, sinkState())).toBe("incomplete");
    expect(notificationSinkSetup(resendSink, null)).toBe("needsCredential");
    expect(notificationSinkSetup(resendSink, sinkState({ secret_configured: false }))).toBe(
      "needsCredential"
    );
    expect(
      notificationSinkSetup(
        {
          ...resendSink,
          targets: { direct_messages: false, mentions: false, keywords: false, watched: [] }
        },
        sinkState()
      )
    ).toBe("needsTargets");
    expect(notificationSinkSetup(resendSink, sinkState())).toBe("configured");
  });
});

describe("watched target labels", () => {
  test("resolves each entry from the room list, never from a stored label", () => {
    const watched: NotificationDeliveryTarget[] = [
      { kind: "room", room_id: "!alpha:example.invalid" },
      { kind: "space", space_id: "!space:example.invalid" },
      { kind: "room", room_id: "!gone:example.invalid" }
    ];
    const rows = notificationTargetsForLabel({
      watched,
      rooms: [room("!alpha:example.invalid", "Alpha room")],
      spaces: [space("!space:example.invalid", "Alpha space")]
    });
    expect(rows).toEqual([
      { kind: "room", id: "!alpha:example.invalid", label: "Alpha room", known: true },
      { kind: "space", id: "!space:example.invalid", label: "Alpha space", known: true },
      { kind: "room", id: "!gone:example.invalid", label: null, known: false }
    ]);
    expect(rows.every((row) => !row.label || !row.label.includes("!"))).toBe(true);
  });
});
