import { t, type MessageId } from "../i18n/messages";
import { roomDisplayLabel } from "./roomDisplayLabel";
import type {
  NotificationDeliveryContent,
  NotificationDeliveryContentConsent,
  NotificationDeliveryOutcomeState,
  NotificationDeliveryPresetState,
  NotificationDeliverySinkState,
  NotificationDeliveryTarget,
  NotificationSinkConfig,
  NotificationSinkSettings,
  RoomSummary,
  SpaceSummary
} from "./types";

/**
 * #1427/#1460: the React-side half of device-local notification delivery.
 *
 * Rust owns the semantics - the dwell, the interval, when a room qualifies, what a body
 * contains, whether a secret exists, and whether a stored consent still covers a sink. The
 * helpers here only mirror the *shape* of a few Rust values that the settings surface has to
 * name back to Rust exactly (a sink's endpoint string, its id alphabet, the numeric bounds)
 * and translate the typed outcome Rust reports. Every mirror names its Rust owner; a drift
 * fails safe, because Rust re-canonicalizes every patch and drops what it cannot recognize.
 */

/** Mirrors `MAX_NOTIFICATION_DELIVERY_DWELL_SECS` and `MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS`. */
export const MAX_NOTIFICATION_DELIVERY_SECONDS = 86_400;
/** Mirrors `DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS`. */
export const DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS = 300;
/** Mirrors `DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS`. */
export const DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS = 900;
/** Mirrors `MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES`. */
export const MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES = 500;
/** Mirrors `DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES`, offered when the mode is chosen. */
export const DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES = 80;
/** Mirrors `NOTIFICATION_DELIVERY_MAIL_TEXT_BUDGET`: Resend and SMTP, per room. */
export const NOTIFICATION_DELIVERY_MAIL_TEXT_BUDGET = 4000;
/** Mirrors `NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET`, per room. */
export const NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET = 1000;
/**
 * Mirrors `MAX_NOTIFICATION_DELIVERY_SINKS`. `canonicalize` keeps the first sinks it
 * recognizes and drops the rest, so a form that offers a ninth would promise a sink Rust
 * silently discards.
 */
export const MAX_NOTIFICATION_DELIVERY_SINKS = 8;
/** Mirrors `MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS`, applied per destination. */
export const MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS = 64;
/** Mirrors `DEFAULT_RESEND_ENDPOINT`, shown so an empty override field is not a mystery. */
export const DEFAULT_RESEND_ENDPOINT = "https://api.resend.com/emails";
/**
 * Mirrors `DEFAULT_RESEND_FROM_ADDRESS`: Resend's shared sender. Rust fills an empty Resend
 * sender with it, so a new Resend destination needs no verified domain to notify yourself.
 */
export const DEFAULT_RESEND_FROM_ADDRESS = "onboarding@resend.dev";

/**
 * Mirrors Rust's default sender name: canonicalization fills an empty `sender_name` with this
 * for Resend and SMTP destinations (ServerChan has no sender and clears it). The field is
 * never required; it is prefilled for a new destination and shown as its placeholder.
 */
export const DEFAULT_NOTIFICATION_SENDER_NAME = "Koushi";
/** Mirrors `DEFAULT_SERVERCHAN_ENDPOINT`. */
export const DEFAULT_SERVERCHAN_ENDPOINT = "https://sctapi.ftqq.com";
/** Where a ServerChan Turbo SendKey is obtained; shown in the setup steps. */
export const SERVERCHAN_SETUP_URL = "https://sct.ftqq.com";

function boundedInteger(value: number, minimum: number, maximum: number): number {
  if (!Number.isFinite(value)) {
    return minimum;
  }
  return Math.min(Math.max(Math.trunc(value), minimum), maximum);
}

/** A dwell or interval the settings form may submit; Rust clamps again on the patch. */
export function clampNotificationDeliverySeconds(value: number): number {
  return boundedInteger(value, 0, MAX_NOTIFICATION_DELIVERY_SECONDS);
}

/** Grapheme clusters of message text the prefix mode carries; never below one while on. */
export function clampNotificationDeliveryPrefixGraphemes(value: number): number {
  return boundedInteger(value, 1, MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES);
}

/**
 * The non-secret string a content consent binds to. Rust
 * `NotificationSinkSettings::endpoint` is the owner: a consent stored for another endpoint
 * is dropped by `canonicalize`, so a drift here asks for the consent again rather than
 * carrying it to a different destination.
 */
export function notificationSinkEndpoint(sink: NotificationSinkSettings): string {
  return sink.config.kind === "smtp"
    ? `${sink.config.host}:${sink.config.port}`
    : sink.config.endpoint;
}

/**
 * The recipient a content consent binds to, mirroring
 * `NotificationSinkSettings::consent_recipient`. For ServerChan it is the revision of the
 * bound SendKey (`sendkey#<revision>`), because the SendKey's owner is the recipient and
 * the settings never see the key; without a revisioned key there is no recipient.
 */
export function notificationConsentRecipient(sink: NotificationSinkSettings): string {
  if (sink.config.kind !== "serverChan") {
    return sink.destination;
  }
  const binding = sink.secret_binding ?? "";
  const hash = binding.lastIndexOf("#");
  const revision = hash >= 0 ? binding.slice(hash + 1) : "";
  return /^[0-9a-fA-F]{1,32}$/.test(revision) ? `sendkey#${revision}` : "";
}

/** The consent that covers this destination exactly as configured now (`consent_for_current`). */
export function notificationConsentForCurrent(
  sink: NotificationSinkSettings,
  acceptedAtMs: number
): NotificationDeliveryContentConsent {
  return {
    sink_kind: sink.config.kind,
    endpoint: notificationSinkEndpoint(sink),
    recipient: notificationConsentRecipient(sink),
    accepted_at_ms: acceptedAtMs
  };
}

/**
 * Whether the stored consent still covers this destination, mirroring
 * `NotificationSinkSettings::consent_matches`. Rust drops a stale consent on every patch,
 * so this only decides what the screen says before Rust's answer arrives.
 */
export function notificationContentConsented(sink: NotificationSinkSettings): boolean {
  const consent = sink.content_consent;
  if (!consent) {
    return false;
  }
  const recipient = notificationConsentRecipient(sink);
  return (
    recipient !== "" &&
    consent.sink_kind === sink.config.kind &&
    consent.endpoint === notificationSinkEndpoint(sink) &&
    consent.recipient === recipient
  );
}

/** The mode this destination actually uses: names only unless the consent matches. */
export function notificationEffectiveContent(
  sink: NotificationSinkSettings
): NotificationDeliveryContent {
  return sink.content.kind !== "namesOnly" && notificationContentConsented(sink)
    ? sink.content
    : { kind: "namesOnly" };
}

/** Mirrors `NotificationSinkSettings::text_budget_graphemes`. */
export function notificationTextBudget(sink: NotificationSinkSettings): number {
  return sink.config.kind === "serverChan"
    ? NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET
    : NOTIFICATION_DELIVERY_MAIL_TEXT_BUDGET;
}

export type ServerChanSendKeyProblem = "serverChan3Key" | "malformed";

/**
 * The shape check `koushi_notify::validate_serverchan_sendkey` applies before a SendKey is
 * stored, repeated so the field can say why before the command leaves. Rust refuses the
 * same values again; a drift can only refuse earlier, never store a key Rust would not.
 */
export function serverChanSendKeyProblem(value: string): ServerChanSendKeyProblem | null {
  const key = value.trim();
  if (key.slice(0, 4).toLowerCase() === "sctp") {
    return "serverChan3Key";
  }
  return /^SCT[A-Za-z0-9]+$/.test(key) && key.length <= 128 ? null : "malformed";
}

/**
 * A sink id from an opaque token. `koushi_key::NotificationSinkSecretId::parse` owns the
 * alphabet and length; a token that survives this is one the credential store can key.
 */
export function notificationSinkIdFromToken(token: string): string {
  const filtered = token.toLowerCase().replace(/[^a-z0-9_-]/g, "");
  return `sink-${filtered}`.slice(0, 64);
}

export function createNotificationSinkId(): string {
  const token =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return notificationSinkIdFromToken(token);
}

export type NotificationSinkKind = NotificationSinkConfig["kind"];

/**
 * The fields a stored credential was entered for, mirroring
 * `NotificationSinkSettings::credential_identity`: the Resend or ServerChan endpoint, or
 * the SMTP host, port, and username. Rust retires the credential when they change, so the
 * detail screen asks for a new secret before such an edit is saved.
 */
export function notificationCredentialIdentity(sink: NotificationSinkSettings): string {
  switch (sink.config.kind) {
    case "resend":
      return `resend:${sink.config.endpoint}`;
    case "serverChan":
      return `serverchan:${sink.config.endpoint}`;
    case "smtp":
      return `smtp:${sink.config.host}:${sink.config.port}:${sink.config.username}`;
  }
}

/**
 * A new destination of `kind` with the conservative defaults Rust gives one
 * (`NotificationDeliveryTargets::default`, `NotificationDeliveryTiming::default`): DMs,
 * mentions and keywords on, no watched targets, names only, 300 s dwell, 900 s interval,
 * only while this app is inactive. Mail fields start empty; the detail view fills them,
 * except a Resend sender, which starts as Resend's shared one, and the sender name, which
 * starts as `DEFAULT_NOTIFICATION_SENDER_NAME` for the mail kinds.
 */
export function newNotificationSink(id: string, kind: NotificationSinkKind): NotificationSinkSettings {
  const config: NotificationSinkConfig =
    kind === "smtp"
      ? { kind: "smtp", preset: null, host: "", port: 465, tls: "implicit", username: "" }
      : kind === "serverChan"
        ? { kind: "serverChan", endpoint: DEFAULT_SERVERCHAN_ENDPOINT }
        : { kind: "resend", endpoint: DEFAULT_RESEND_ENDPOINT };
  return {
    id,
    destination: "",
    from_address: kind === "resend" ? DEFAULT_RESEND_FROM_ADDRESS : "",
    sender_name: kind === "serverChan" ? "" : DEFAULT_NOTIFICATION_SENDER_NAME,
    config,
    // Nothing is stored for a sink that has never had a credential: the binding Rust
    // records is the one attached to whatever the user saves next.
    secret_binding: null,
    targets: { direct_messages: true, mentions: true, keywords: true, watched: [] },
    content: { kind: "namesOnly" },
    content_consent: null,
    timing: {
      dwell_secs: DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS,
      min_interval_secs: DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS,
      only_when_unfocused: true
    },
    direct_partner_label: "localAlias"
  };
}

/**
 * Whether the non-secret setup `NotificationSinkSettings::is_usable` requires is filled
 * in. Rust drops a sink that fails it, so the add flow only commits a sink that passes.
 * A Resend sender is optional: Rust fills an empty one with `DEFAULT_RESEND_FROM_ADDRESS`.
 */
export function notificationSinkFieldsReady(sink: NotificationSinkSettings): boolean {
  switch (sink.config.kind) {
    case "serverChan":
      return true;
    case "resend":
      return Boolean(sink.destination.trim());
    case "smtp":
      return Boolean(
        sink.destination.trim() &&
          sink.from_address.trim() &&
          sink.config.host.trim() &&
          sink.config.port > 0
      );
  }
}

/**
 * Setup completeness as the list row and the outcome block state it. "Configured" is
 * setup only - fields, a credential bound to them, and at least one target - and never a
 * claim that anything was delivered.
 */
export type NotificationSinkSetup = "configured" | "needsCredential" | "needsTargets" | "incomplete";

export function notificationSinkSetup(
  sink: NotificationSinkSettings,
  sinkState: NotificationDeliverySinkState | null
): NotificationSinkSetup {
  if (!notificationSinkFieldsReady(sink)) {
    return "incomplete";
  }
  if (!sinkState?.secret_configured) {
    return "needsCredential";
  }
  return notificationTargetsAny(sink) ? "configured" : "needsTargets";
}

export function notificationTargetsAny(sink: NotificationSinkSettings): boolean {
  const targets = sink.targets;
  return (
    targets.direct_messages || targets.mentions || targets.keywords || targets.watched.length > 0
  );
}

/**
 * What the form fills in when a mailbox preset is chosen. The preset is Rust's table; the
 * username keeps the shape the vendor documents (`username_is_full_address`), and the user
 * may still replace it by hand.
 */
export function notificationSmtpConfigFromPreset(
  preset: NotificationDeliveryPresetState,
  fromAddress: string
): Extract<NotificationSinkConfig, { kind: "smtp" }> {
  return {
    kind: "smtp",
    preset: preset.id,
    host: preset.host,
    port: preset.port,
    tls: preset.tls,
    username: preset.username_is_full_address ? fromAddress.trim() : ""
  };
}

/**
 * The service name the list and the consent copy name. A preset's `label` is the vendor's own proper
 * name and is deliberately not localized; the generic kinds are catalog prose.
 */
export function notificationDeliveryServiceLabel(
  sink: NotificationSinkSettings,
  presets: readonly NotificationDeliveryPresetState[]
): string {
  if (sink.config.kind === "smtp") {
    const presetId = sink.config.preset ?? null;
    const preset = presets.find((candidate) => candidate.id === presetId);
    if (preset) {
      return preset.label;
    }
    return t("settings.notificationDeliverySinkKindSmtp");
  }
  if (sink.config.kind === "serverChan") {
    return t("settings.notificationDeliverySinkKindServerChan");
  }
  return t("settings.notificationDeliverySinkKindResend");
}

export interface NotificationOutcomeMessage {
  id: MessageId;
  values?: Record<string, string | number>;
}

/** The catalog text of one sink's last outcome, including a rate limit's wait. */
export function notificationDeliveryOutcomeMessage(
  outcome: NotificationDeliveryOutcomeState | null
): NotificationOutcomeMessage | null {
  if (outcome === null) {
    return null;
  }
  switch (outcome.kind) {
    case "accepted":
      return { id: "settings.notificationDeliveryResultAccepted" };
    case "invalidSecret":
      return { id: "settings.notificationDeliveryResultInvalidSecret" };
    case "notPermitted":
      return { id: "settings.notificationDeliveryResultNotPermitted" };
    case "rejected":
      return { id: "settings.notificationDeliveryResultRejected" };
    case "transport":
      return { id: "settings.notificationDeliveryResultTransport" };
    case "rateLimited":
      return outcome.retry_after_secs === null
        ? { id: "settings.notificationDeliveryResultRateLimited" }
        : {
            id: "settings.notificationDeliveryResultRateLimitedAfter",
            values: { seconds: outcome.retry_after_secs }
          };
  }
}

/** Whether Rust last reported a send through this sink as anything but accepted. */
export function notificationDeliverySinkFailed(
  outcome: NotificationDeliveryOutcomeState | null
): boolean {
  return outcome !== null && outcome.kind !== "accepted";
}

export interface NotificationTargetLabel {
  kind: "room" | "space";
  id: string;
  /** The room list's own display name, resolved now; settings store ids only. */
  label: string | null;
  known: boolean;
}

/**
 * Resolve each watch-list entry's label from the room list. Settings hold ids only, so a
 * renamed room shows its current name and an entry the list no longer has is reported as
 * unknown rather than rendered as a bare Matrix id.
 */
export function notificationTargetsForLabel({
  watched,
  rooms,
  spaces
}: {
  watched: readonly NotificationDeliveryTarget[];
  rooms: readonly RoomSummary[];
  spaces: readonly SpaceSummary[];
}): NotificationTargetLabel[] {
  const roomLabels = new Map(rooms.map((room) => [room.room_id, roomDisplayLabel(room)]));
  const spaceLabels = new Map(spaces.map((space) => [space.space_id, space.display_name]));
  return watched.map((target) => {
    if (target.kind === "room") {
      const label = roomLabels.get(target.room_id) ?? null;
      return { kind: "room" as const, id: target.room_id, label, known: label !== null };
    }
    const label = spaceLabels.get(target.space_id) ?? null;
    return { kind: "space" as const, id: target.space_id, label, known: label !== null };
  });
}
