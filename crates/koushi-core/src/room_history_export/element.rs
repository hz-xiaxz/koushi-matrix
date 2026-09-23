//! Element-compatible chat-export JSON (#59).
//!
//! This is a port of the parts of Element Web's `JSONExporter` that decide the
//! file shape, pinned to element-web `c9cff69c74d5faa4606167863e066ef02c0bae0d`
//! and matrix-js-sdk `b08a603df74fbb7e97cbfe83097b004ff4122b93`:
//!
//! - `messages[]` holds `MatrixEvent.getEffectiveEvent()` for every fetched
//!   event that passes `haveRendererForEvent(event, client, false)`
//!   (`EventTileFactory.tsx`, `TextForEvent.tsx`), in chronological order.
//! - The top-level object is `room_name`, `room_creator`, `topic`,
//!   `export_date`, `exported_by`, `messages`, serialized like
//!   `JSON.stringify(object, null, 2)`.
//!
//! Element's exporter maps freshly fetched `/messages` events: an edit is
//! applied to its original only when the server bundles the complete edit
//! event, and `m.replace` events themselves are dropped by the renderer
//! filter. This module reproduces that behaviour rather than normalizing
//! history into a Koushi-specific shape.
//!
//! Nothing here performs I/O or talks to the SDK: callers classify each event
//! into an [`ExportSourceEvent`] and write through [`ElementJsonWriter`].

use serde_json::{Map, Value};

/// One event as fetched from `/messages`, classified by decryption outcome.
#[derive(Clone)]
pub(crate) enum ExportSourceEvent {
    /// A plaintext event, verbatim from the server.
    Plain(Value),
    /// A decrypted event as produced by the Matrix crypto crate: the plaintext
    /// payload merged with the wire `sender`, `event_id`, `origin_server_ts`,
    /// `unsigned`, and wire `m.relates_to`. This is the same object
    /// matrix-js-sdk stores as `clearEvent` for its Rust crypto backend.
    Decrypted(Value),
    /// An `m.room.encrypted` event this device could not decrypt.
    Undecryptable {
        wire: Value,
        reason: UndecryptableReason,
    },
}

/// The subset of decryption-failure reasons Element words differently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UndecryptableReason {
    MissingRoomKey,
    UnknownMessageIndex,
    Other,
}

impl UndecryptableReason {
    /// `String(DecryptionError)` for the matching matrix-js-sdk Rust-crypto
    /// failure; the exact reason text is dynamic data in Element's output.
    fn element_text(self) -> &'static str {
        match self {
            Self::MissingRoomKey => {
                "DecryptionError: The sender's device has not sent us the keys for this message."
            }
            Self::UnknownMessageIndex => {
                "DecryptionError: The sender's device has not sent us the keys for this message at this index."
            }
            Self::Other => "DecryptionError: Unknown error",
        }
    }
}

/// An event mapped to Element's effective-event representation.
#[derive(Clone)]
pub(crate) struct EffectiveEvent {
    pub(crate) json: Value,
    /// The wire event was `m.room.encrypted`.
    pub(crate) encrypted: bool,
    /// Exported as Element's `m.bad.encrypted` placeholder.
    pub(crate) undecryptable: bool,
}

impl EffectiveEvent {
    pub(crate) fn event_id(&self) -> Option<&str> {
        self.json.get("event_id").and_then(Value::as_str)
    }

    pub(crate) fn origin_server_ts(&self) -> Option<u64> {
        self.json.get("origin_server_ts").and_then(Value::as_u64)
    }
}

const ENCRYPTED_TYPE: &str = "m.room.encrypted";
/// Wire-content keys `getEffectiveEvent()` never copies into the content.
const ENCRYPTION_SCHEMA_KEYS: [&str; 5] = [
    "algorithm",
    "ciphertext",
    "device_id",
    "sender_key",
    "session_id",
];

fn is_redacted(event: &Value) -> bool {
    event
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("redacted_because"))
        .is_some_and(|because| !because.is_null())
}

/// `MatrixEvent.getEffectiveEvent()` for one fetched event.
pub(crate) fn effective_event(source: ExportSourceEvent) -> EffectiveEvent {
    let mut event = map_decryption(source);
    apply_bundled_replacement(&mut event);
    event
}

/// matrix-js-sdk's event mapper applies a complete edit bundled by the server
/// (`unsigned["m.relations"]["m.replace"]` with `content`) through
/// `makeReplaced`, after which `getContent()` is the edit's `m.new_content`,
/// or `{}` without one. Redacted and state events are never replaced.
///
/// An encrypted bundled edit this device could not decrypt keeps the original
/// content: Element decrypts the replacement in the background, so its result
/// for that case depends on timing.
fn apply_bundled_replacement(event: &mut EffectiveEvent) {
    if event.undecryptable || is_redacted(&event.json) || event.json.get("state_key").is_some() {
        return;
    }
    let Some(bundled) = event
        .json
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("m.relations"))
        .and_then(|relations| relations.get("m.replace"))
    else {
        return;
    };
    let Some(bundled_content) = bundled.get("content").and_then(Value::as_object) else {
        return;
    };
    if event_type(bundled) == Some(ENCRYPTED_TYPE) {
        return;
    }
    let mut replaced = bundled_content
        .get("m.new_content")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // For an encrypted original, `getEffectiveEvent()` copies wire content keys
    // outside the encryption schema, which the crypto crate already moved into
    // the decrypted content (the wire `m.relates_to`).
    if event.encrypted
        && let Some(relation) = content(&event.json).get("m.relates_to")
        && !replaced.contains_key("m.relates_to")
    {
        replaced.insert("m.relates_to".to_owned(), relation.clone());
    }
    if let Some(object) = event.json.as_object_mut() {
        object.insert("content".to_owned(), Value::Object(replaced));
    }
}

fn map_decryption(source: ExportSourceEvent) -> EffectiveEvent {
    match source {
        // The SDK returns an `m.room.encrypted` event it could not attempt to
        // decrypt (malformed content, or no crypto store) as plaintext;
        // matrix-js-sdk turns the same failure into `m.bad.encrypted`.
        ExportSourceEvent::Plain(json)
            if event_type(&json) == Some(ENCRYPTED_TYPE) && !is_redacted(&json) =>
        {
            map_decryption(ExportSourceEvent::Undecryptable {
                wire: json,
                reason: UndecryptableReason::Other,
            })
        }
        ExportSourceEvent::Plain(json) => {
            let encrypted = event_type(&json) == Some(ENCRYPTED_TYPE);
            EffectiveEvent {
                json,
                encrypted,
                undecryptable: false,
            }
        }
        ExportSourceEvent::Decrypted(json) => EffectiveEvent {
            json,
            encrypted: true,
            undecryptable: false,
        },
        // matrix-js-sdk never attempts to decrypt a redacted event, so it stays
        // the pruned `m.room.encrypted` wire event.
        ExportSourceEvent::Undecryptable { wire, .. } if is_redacted(&wire) => EffectiveEvent {
            json: wire,
            encrypted: true,
            undecryptable: false,
        },
        ExportSourceEvent::Undecryptable { mut wire, reason } => {
            let mut content = Map::new();
            content.insert("msgtype".to_owned(), Value::from("m.bad.encrypted"));
            content.insert(
                "body".to_owned(),
                Value::from(format!(
                    "** Unable to decrypt: {} **",
                    reason.element_text()
                )),
            );
            if let Some(wire_content) = wire.get("content").and_then(Value::as_object) {
                for (key, value) in wire_content {
                    if !ENCRYPTION_SCHEMA_KEYS.contains(&key.as_str()) && !content.contains_key(key)
                    {
                        content.insert(key.clone(), value.clone());
                    }
                }
            }
            if let Some(object) = wire.as_object_mut() {
                object.insert("type".to_owned(), Value::from("m.room.message"));
                object.insert("content".to_owned(), Value::Object(content));
            }
            EffectiveEvent {
                json: wire,
                encrypted: true,
                undecryptable: true,
            }
        }
    }
}

fn event_type(event: &Value) -> Option<&str> {
    event.get("type").and_then(Value::as_str)
}

fn content(event: &Value) -> &Value {
    static EMPTY: Value = Value::Null;
    event.get("content").unwrap_or(&EMPTY)
}

fn prev_content(event: &Value) -> &Value {
    static EMPTY: Value = Value::Null;
    event
        .get("unsigned")
        .and_then(|unsigned| unsigned.get("prev_content"))
        .unwrap_or(&EMPTY)
}

fn field<'a>(object: &'a Value, key: &str) -> Option<&'a Value> {
    object.get(key).filter(|value| !value.is_null())
}

fn field_str<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

/// JavaScript truthiness of an optional JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

const MESSAGE_TILE_TYPES: [&str; 8] = [
    "m.room.message",
    "m.sticker",
    "m.poll.start",
    "org.matrix.msc3381.poll.start",
    "m.poll.end",
    "org.matrix.msc3381.poll.end",
    "m.call.invite",
    "org.matrix.msc4075.rtc.notification",
];

/// `isMessageEvent`: messages, stickers, and polls.
const MESSAGE_EVENT_TYPES: [&str; 6] = [
    "m.room.message",
    "m.sticker",
    "m.poll.start",
    "org.matrix.msc3381.poll.start",
    "m.poll.end",
    "org.matrix.msc3381.poll.end",
];

const POLICY_RULE_TYPES: [&str; 9] = [
    "m.policy.rule.user",
    "m.room.rule.user",
    "org.matrix.mjolnir.rule.user",
    "m.policy.rule.room",
    "m.room.rule.room",
    "org.matrix.mjolnir.rule.room",
    "m.policy.rule.server",
    "m.room.rule.server",
    "org.matrix.mjolnir.rule.server",
];

const SINGULAR_STATE_TYPES: [&str; 14] = [
    "m.room.encryption",
    "m.room.canonical_alias",
    "m.room.create",
    "m.room.name",
    "m.room.avatar",
    "m.room.history_visibility",
    "m.room.topic",
    "m.room.power_levels",
    "m.room.pinned_events",
    "m.room.server_acl",
    "io.element.widgets.layout",
    "m.room.tombstone",
    "m.room.join_rules",
    "m.room.guest_access",
];

const GROUP_CALL_TYPE: &str = "org.matrix.msc3401.call";

/// State tile factories that are not `TextualEventFactory`.
fn is_non_textual_state_tile(event_type: &str) -> bool {
    matches!(
        event_type,
        "m.room.encryption" | "m.room.create" | "m.room.avatar" | GROUP_CALL_TYPE
    )
}

/// `STATE_EVENT_TILE_TYPES` entries backed by `TextualEventFactory`.
fn is_textual_state_tile(event_type: &str) -> bool {
    matches!(
        event_type,
        "m.room.canonical_alias"
            | "m.room.member"
            | "m.room.name"
            | "m.room.third_party_invite"
            | "m.room.history_visibility"
            | "m.room.topic"
            | "m.room.power_levels"
            | "m.room.pinned_events"
            | "m.room.server_acl"
            | "im.vector.modular.widgets"
            | "io.element.widgets.layout"
            | "m.room.tombstone"
            | "m.room.join_rules"
            | "m.room.guest_access"
    ) || POLICY_RULE_TYPES.contains(&event_type)
}

/// `isRelation(RelationType.Replace)`, which reads the wire relation. Element
/// moves the relation to the cleartext wire content of encrypted events and
/// the crypto crate copies it into the decrypted content.
fn is_replacement(event: &Value, is_state: bool) -> bool {
    if is_state {
        return false;
    }
    let Some(relation) = field(content(event), "m.relates_to") else {
        return false;
    };
    field_str(relation, "rel_type") == Some("m.replace") && truthy(field(relation, "event_id"))
}

fn modification(previous: Option<&Value>, next: Option<&Value>) -> Modification {
    match (truthy(previous), truthy(next)) {
        (true, true) if previous != next => Modification::Changed,
        (true, false) => Modification::Unset,
        (false, true) => Modification::Set,
        _ => Modification::None,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Modification {
    None,
    Set,
    Changed,
    Unset,
}

/// `textForMemberEvent` returns a renderer, with hidden events and the
/// `feature_ask_to_join` lab both at their defaults (off).
fn member_has_text(event: &Value) -> bool {
    let content = content(event);
    let previous = prev_content(event);
    let previous_membership = field_str(previous, "membership");
    match field_str(content, "membership") {
        Some("invite" | "ban") => true,
        Some("join") if previous_membership == Some("join") => {
            modification(
                field(previous, "displayname"),
                field(content, "displayname"),
            ) != Modification::None
                || modification(field(previous, "avatar_url"), field(content, "avatar_url"))
                    != Modification::None
        }
        Some("join") => true,
        Some("leave") => {
            field_str(event, "sender") == field_str(event, "state_key")
                || matches!(previous_membership, Some("ban" | "invite" | "join"))
        }
        _ => false,
    }
}

/// `textForPowerEvent` only renders a change in some user's level.
fn power_levels_have_text(event: &Value) -> bool {
    let (Some(previous_users), Some(users)) = (
        field(prev_content(event), "users").filter(|users| truthy(Some(users))),
        field(content(event), "users").filter(|users| truthy(Some(users))),
    ) else {
        return false;
    };
    let level_default = |object: &Value| -> f64 {
        field(object, "users_default")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    let previous_default = level_default(prev_content(event));
    let current_default = level_default(content(event));
    let level = |users: &Value, user_id: &str, default: f64| -> f64 {
        users
            .get(user_id)
            .and_then(Value::as_f64)
            .filter(|level| level.fract() == 0.0)
            .unwrap_or(default)
    };
    let user_ids = users
        .as_object()
        .into_iter()
        .chain(previous_users.as_object())
        .flat_map(|object| object.keys());
    for user_id in user_ids {
        let from = level(previous_users, user_id, previous_default);
        let to = level(users, user_id, current_default);
        if from == previous_default && to == current_default {
            continue;
        }
        if to != from {
            return true;
        }
    }
    false
}

/// `hasText` for the textual state factories.
fn textual_state_has_text(event: &Value, event_type: &str) -> bool {
    match event_type {
        "m.room.member" => member_has_text(event),
        "m.room.power_levels" => power_levels_have_text(event),
        // `STATE_EVENT_TILE_TYPES` lists it, but `TextForEvent` has no handler.
        "m.room.server_acl" => false,
        _ => true,
    }
}

fn is_beacon_info(event_type: &str) -> bool {
    matches!(
        event_type,
        "m.beacon_info" | "org.matrix.msc3672.beacon_info"
    )
}

/// `haveRendererForEvent(event, client, showHiddenEvents = false)`.
///
/// `own_user_id` decides whether a verification request involves this
/// account. Element's module hints and moderation lab are not active by
/// default and have no counterpart here.
pub(crate) fn element_renders(event: &EffectiveEvent, own_user_id: &str) -> bool {
    let json = &event.json;
    let Some(event_type) = event_type(json) else {
        return false;
    };
    let is_state = json.get("state_key").is_some();
    let redacted = is_redacted(json);
    if redacted && !event.encrypted && !MESSAGE_EVENT_TYPES.contains(&event_type) && !is_state {
        return false;
    }
    if is_replacement(json, is_state) {
        return false;
    }

    // `pickFactory`.
    if event_type == "m.room.message"
        && field_str(content(json), "msgtype") == Some("m.key.verification.request")
    {
        return field_str(json, "sender") == Some(own_user_id)
            || field_str(content(json), "to") == Some(own_user_id);
    }
    if event_type == "m.room.create" {
        let has_predecessor = field(content(json), "predecessor")
            .and_then(|predecessor| field_str(predecessor, "room_id"))
            .is_some();
        if !has_predecessor {
            return false;
        }
    }
    if event_type == "im.vector.modular.widgets" {
        let widget_type = field_str(content(json), "type")
            .filter(|kind| !kind.is_empty())
            .or_else(|| field_str(prev_content(json), "type"));
        if matches!(widget_type, Some("m.jitsi" | "jitsi")) {
            return true;
        }
    }
    if is_state {
        if is_beacon_info(event_type) && (truthy(field(content(json), "live")) || redacted) {
            return true;
        }
        if SINGULAR_STATE_TYPES.contains(&event_type) && field_str(json, "state_key") != Some("") {
            return false;
        }
        if is_textual_state_tile(event_type) {
            return textual_state_has_text(json, event_type);
        }
        if event_type == GROUP_CALL_TYPE {
            let newly_started = prev_content(json).as_object().is_none_or(Map::is_empty);
            let intent = field_str(content(json), "m.intent");
            return newly_started && intent.is_some_and(|intent| intent != "m.room");
        }
        return is_non_textual_state_tile(event_type);
    }
    if redacted {
        return true;
    }
    MESSAGE_TILE_TYPES.contains(&event_type)
}

/// Element's top-level metadata. `room_creator` is omitted when the room has
/// no create event, as `JSON.stringify` drops `undefined`.
#[derive(Clone)]
pub(crate) struct ExportHeader {
    pub(crate) room_name: String,
    pub(crate) room_creator: Option<String>,
    pub(crate) topic: String,
    pub(crate) export_date: String,
    pub(crate) exported_by: String,
}

/// Calendar locales for Element's `formatFullDateNoDayNoTime`, i.e.
/// `Intl.DateTimeFormat(locale, { year, month, day: "numeric" })`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExportDateLocale {
    /// `en`: `M/D/YYYY`.
    En,
    /// `ja`: `YYYY/M/D`.
    Ja,
}

impl From<koushi_state::CatalogLocale> for ExportDateLocale {
    fn from(locale: koushi_state::CatalogLocale) -> Self {
        match locale {
            koushi_state::CatalogLocale::Ja => Self::Ja,
            koushi_state::CatalogLocale::En | koushi_state::CatalogLocale::Pseudo => Self::En,
        }
    }
}

/// Civil date for days since 1970-01-01 (proleptic Gregorian).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Element's `export_date` for `now_ms` in the platform's current UTC offset.
pub(crate) fn format_export_date(
    now_ms: u64,
    utc_offset_minutes: i32,
    locale: ExportDateLocale,
) -> String {
    let local_ms = i128::from(now_ms) + i128::from(utc_offset_minutes) * 60_000;
    let days = local_ms.div_euclid(86_400_000) as i64;
    let (year, month, day) = civil_from_days(days);
    match locale {
        ExportDateLocale::En => format!("{month}/{day}/{year}"),
        ExportDateLocale::Ja => format!("{year}/{month}/{day}"),
    }
}

/// Pretty-print one value as `JSON.stringify(value, null, 2)` nested `depth`
/// levels deep. JSON strings never contain raw newlines, so indenting each
/// line is exact.
fn pretty_nested(value: &Value, depth: usize) -> String {
    let text = serde_json::to_string_pretty(value).expect("JSON values always serialize");
    let indent = "  ".repeat(depth);
    let mut out = String::with_capacity(text.len() + indent.len() * 8);
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push('\n');
            out.push_str(&indent);
        }
        out.push_str(line);
    }
    out
}

/// Streams one Element export: the header on creation, then one event at a
/// time, then the closing brackets. The caller owns the byte sink so no event
/// is retained after it is written.
pub(crate) struct ElementJsonWriter {
    written_events: u64,
}

impl ElementJsonWriter {
    pub(crate) fn begin(header: &ExportHeader) -> (Self, Vec<u8>) {
        let mut out = String::from("{\n");
        let mut push = |key: &str, value: &str| {
            out.push_str("  ");
            out.push_str(&Value::from(key).to_string());
            out.push_str(": ");
            out.push_str(&Value::from(value).to_string());
            out.push_str(",\n");
        };
        push("room_name", &header.room_name);
        if let Some(creator) = &header.room_creator {
            push("room_creator", creator);
        }
        push("topic", &header.topic);
        push("export_date", &header.export_date);
        push("exported_by", &header.exported_by);
        out.push_str("  \"messages\": [");
        (Self { written_events: 0 }, out.into_bytes())
    }

    pub(crate) fn event(&mut self, event: &Value) -> Vec<u8> {
        let mut out = String::from(if self.written_events == 0 {
            "\n    "
        } else {
            ",\n    "
        });
        out.push_str(&pretty_nested(event, 2));
        self.written_events += 1;
        out.into_bytes()
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        if self.written_events == 0 {
            b"]\n}".to_vec()
        } else {
            b"\n  ]\n}".to_vec()
        }
    }
}
