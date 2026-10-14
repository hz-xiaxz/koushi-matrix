import { t, type MessageId } from "../i18n/messages";
import { roomDisplayLabel } from "./roomDisplayLabel";
import type {
  NotificationDeliveryOutcomeState,
  NotificationDeliveryPresetState,
  NotificationDeliverySettings,
  NotificationDeliveryTarget,
  NotificationSinkConfig,
  NotificationSinkSettings,
  RoomSummary,
  SmtpTls,
  SpaceSummary
} from "./types";

/**
 * #1427: the React-side half of device-local notification delivery.
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
/** Mirrors `MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS`. */
export const MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS = 200;
/**
 * Mirrors `MAX_NOTIFICATION_DELIVERY_SINKS`. `canonicalize` keeps the first sinks it
 * recognizes and drops the rest, so a form that offers a ninth would promise a sink Rust
 * silently discards.
 */
export const MAX_NOTIFICATION_DELIVERY_SINKS = 8;
/** Mirrors `MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS`, applied the same way. */
export const MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS = 64;
/** Mirrors `DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS`, restored when the option is turned on. */
export const DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS = 80;

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

/** Characters of message text a prefix consent would expose; never below one while on. */
export function clampNotificationDeliveryPrefixChars(value: number): number {
  return boundedInteger(value, 1, MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS);
}

/**
 * The non-secret string a prefix consent binds to. Rust
 * `NotificationSinkSettings::endpoint` is the owner: a consent stored for another endpoint
 * is dropped by `NotificationDeliverySettings::canonicalize`, so a drift here asks for the
 * consent again rather than carrying it to a different destination.
 */
export function notificationSinkEndpoint(sink: NotificationSinkSettings): string {
  return sink.config.kind === "resend"
    ? sink.config.endpoint
    : `${sink.config.host}:${sink.config.port}`;
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

/** The add form's non-secret working copy of one sink, before Rust sees it. */
export interface NotificationSinkDraft {
  kind: "resend" | "smtp";
  from_address: string;
  sender_name: string;
  destination: string;
  endpoint: string;
  preset: string | null;
  host: string;
  port: number;
  tls: SmtpTls;
  username: string;
}

export function emptyNotificationSinkDraft(): NotificationSinkDraft {
  return {
    kind: "resend",
    from_address: "",
    sender_name: "",
    destination: "",
    endpoint: "",
    preset: null,
    host: "",
    port: 465,
    tls: "implicit",
    username: ""
  };
}

/**
 * Whether the add form carries the fields `NotificationSinkSettings::is_usable` requires, so
 * an "Add" a user can press never silently disappears in canonicalization. The id and the
 * Resend endpoint are Rust's to fill: an empty endpoint becomes `DEFAULT_RESEND_ENDPOINT`.
 */
export function notificationDraftReady(draft: NotificationSinkDraft): boolean {
  if (!draft.destination.trim() || !draft.from_address.trim()) {
    return false;
  }
  return draft.kind === "resend" ? true : Boolean(draft.host.trim()) && draft.port > 0;
}

export function notificationSinkFromDraft(id: string, draft: NotificationSinkDraft): NotificationSinkSettings {
  const config: NotificationSinkConfig =
    draft.kind === "resend"
      ? { kind: "resend", endpoint: draft.endpoint.trim() }
      : {
          kind: "smtp",
          preset: draft.preset,
          host: draft.host.trim(),
          port: draft.port,
          tls: draft.tls,
          username: draft.username.trim()
        };
  return {
    id,
    enabled: false,
    destination: draft.destination.trim(),
    from_address: draft.from_address.trim(),
    sender_name: draft.sender_name.trim(),
    config
  };
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
 * The service name the consent copy names. A preset's `label` is the vendor's own proper
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
    case "delivered":
      return { id: "settings.notificationDeliveryResultDelivered" };
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

/** Whether Rust last reported a send through this sink as anything but delivered. */
export function notificationDeliverySinkFailed(
  outcome: NotificationDeliveryOutcomeState | null
): boolean {
  return outcome !== null && outcome.kind !== "delivered";
}

/**
 * Whether the stored consent covers this exact sink, destination and endpoint, mirroring
 * `NotificationDeliverySettings::prefix_is_allowed_for`. A mismatch is what turns the option
 * back off after a sink, endpoint or destination change.
 */
export function notificationDeliveryPrefixConsented(
  delivery: NotificationDeliverySettings,
  sink: NotificationSinkSettings
): boolean {
  const consent = delivery.prefix_consent;
  return (
    delivery.prefix_chars > 0 &&
    consent !== null &&
    consent !== undefined &&
    consent.sink_id === sink.id &&
    consent.destination === sink.destination &&
    consent.endpoint === notificationSinkEndpoint(sink)
  );
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
