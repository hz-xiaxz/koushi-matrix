import { afterEach, describe, expect, test } from "vitest";

import {
  MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS,
  MAX_NOTIFICATION_DELIVERY_SECONDS,
  clampNotificationDeliveryPrefixChars,
  clampNotificationDeliverySeconds,
  notificationDeliveryOutcomeMessage,
  notificationDeliveryPrefixConsented,
  notificationDeliveryServiceLabel,
  notificationDeliverySinkFailed,
  notificationDraftReady,
  notificationSinkEndpoint,
  notificationSinkFromDraft,
  notificationSinkIdFromToken,
  notificationSmtpConfigFromPreset,
  notificationTargetsForLabel
} from "./notificationDelivery";
import { setActiveLocaleProfile } from "../i18n/messages";
import type {
  NotificationDeliveryPrefixConsent,
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationSinkConfig,
  NotificationSinkSettings,
  NotificationDeliveryTarget,
  RoomSummary,
  SpaceSummary
} from "./types";

afterEach(() => setActiveLocaleProfile("en", "none"));

const resendSink: NotificationSinkSettings = {
  id: "sink-one",
  enabled: true,
  destination: "recipient@example.invalid",
  from_address: "sender@example.invalid",
  sender_name: "",
  config: { kind: "resend", endpoint: "https://api.example.invalid/emails" }
};

const smtpSink: NotificationSinkSettings = {
  id: "sink-two",
  enabled: false,
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

function delivery(overrides: Partial<NotificationDeliverySettings> = {}): NotificationDeliverySettings {
  return {
    enabled: true,
    direct_messages: true,
    mentions: true,
    keywords: true,
    only_when_unfocused: true,
    dwell_secs: 300,
    min_interval_secs: 900,
    prefix_chars: 80,
    prefix_consent: null,
    watched: [],
    sinks: [resendSink, smtpSink],
    ...overrides
  };
}

function consent(overrides: Partial<NotificationDeliveryPrefixConsent> = {}): NotificationDeliveryPrefixConsent {
  return {
    sink_id: resendSink.id,
    destination: resendSink.destination,
    endpoint: notificationSinkEndpoint(resendSink),
    accepted_at_ms: 1_700_000_000_000,
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

  test("clamps the plaintext prefix to the Rust maximum and to at least one character", () => {
    expect(MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS).toBe(200);
    expect(clampNotificationDeliveryPrefixChars(5_000)).toBe(200);
    expect(clampNotificationDeliveryPrefixChars(80)).toBe(80);
    expect(clampNotificationDeliveryPrefixChars(0)).toBe(1);
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
  test("mirrors the non-secret endpoint a prefix consent binds to", () => {
    expect(notificationSinkEndpoint(resendSink)).toBe("https://api.example.invalid/emails");
    expect(notificationSinkEndpoint(smtpSink)).toBe("smtp.example.invalid:465");
  });
});

describe("notification delivery outcomes", () => {
  test("translates every typed outcome, including the rate-limit wait", () => {
    expect(notificationDeliveryOutcomeMessage(null)).toBeNull();
    expect(notificationDeliveryOutcomeMessage({ kind: "delivered", at_ms: 1 })?.id).toBe(
      "settings.notificationDeliveryResultDelivered"
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

  test("only a delivered outcome leaves a sink looking healthy", () => {
    expect(notificationDeliverySinkFailed(null)).toBe(false);
    expect(notificationDeliverySinkFailed({ kind: "delivered", at_ms: 1 })).toBe(false);
    for (const kind of ["invalidSecret", "notPermitted", "rejected", "transport"] as const) {
      expect(notificationDeliverySinkFailed({ kind, at_ms: 1 })).toBe(true);
    }
    expect(
      notificationDeliverySinkFailed({ kind: "rateLimited", at_ms: 1, retry_after_secs: 60 })
    ).toBe(true);
  });
});

describe("plaintext prefix consent", () => {
  test("honours a consent only for the same sink, destination and endpoint", () => {
    expect(
      notificationDeliveryPrefixConsented(delivery({ prefix_consent: consent() }), resendSink)
    ).toBe(true);
    expect(
      notificationDeliveryPrefixConsented(
        delivery({ prefix_consent: consent({ destination: "other@example.invalid" }) }),
        resendSink
      )
    ).toBe(false);
    expect(
      notificationDeliveryPrefixConsented(
        delivery({ prefix_consent: consent({ sink_id: "sink-other" }) }),
        resendSink
      )
    ).toBe(false);
    expect(
      notificationDeliveryPrefixConsented(
        delivery({ prefix_consent: consent({ endpoint: "https://elsewhere.example.invalid/x" }) }),
        resendSink
      )
    ).toBe(false);
    // A stored consent with no character count is off, whatever it names.
    expect(
      notificationDeliveryPrefixConsented(
        delivery({ prefix_chars: 0, prefix_consent: consent() }),
        resendSink
      )
    ).toBe(false);
    expect(notificationDeliveryPrefixConsented(delivery(), resendSink)).toBe(false);
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
  test("builds a complete sink from the add form so Rust keeps it", () => {
    const sink = notificationSinkFromDraft("sink-new", {
      kind: "smtp",
      from_address: "sender@example.invalid",
      sender_name: "Koushi",
      destination: "recipient@example.invalid",
      endpoint: "",
      preset: "gmail",
      host: "smtp.example.invalid",
      port: 587,
      tls: "startTls",
      username: "sender@example.invalid"
    });
    expect(sink).toEqual({
      id: "sink-new",
      enabled: false,
      destination: "recipient@example.invalid",
      from_address: "sender@example.invalid",
      sender_name: "Koushi",
      config: {
        kind: "smtp",
        preset: "gmail",
        host: "smtp.example.invalid",
        port: 587,
        tls: "startTls",
        username: "sender@example.invalid"
      }
    });
    // The Resend endpoint is left to Rust's default when the form leaves it empty.
    expect(
      notificationSinkFromDraft("sink-two", {
        kind: "resend",
        from_address: "sender@example.invalid",
        sender_name: "",
        destination: "recipient@example.invalid",
        endpoint: "",
        preset: null,
        host: "",
        port: 0,
        tls: "implicit",
        username: ""
      }).config
    ).toEqual({ kind: "resend", endpoint: "" });
  });

  test("only offers to add a sink Rust would keep", () => {
    const base = {
      kind: "resend" as const,
      from_address: "sender@example.invalid",
      sender_name: "",
      destination: "recipient@example.invalid",
      endpoint: "",
      preset: null,
      host: "",
      port: 0,
      tls: "implicit" as const,
      username: ""
    };
    expect(notificationDraftReady(base)).toBe(true);
    expect(notificationDraftReady({ ...base, destination: "  " })).toBe(false);
    expect(notificationDraftReady({ ...base, from_address: "" })).toBe(false);
    expect(notificationDraftReady({ ...base, kind: "smtp" })).toBe(false);
    expect(notificationDraftReady({ ...base, kind: "smtp", host: "smtp.example.invalid", port: 465 })).toBe(
      true
    );
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
