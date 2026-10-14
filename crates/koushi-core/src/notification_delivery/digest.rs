//! The message body and the per-sink delivery intents built from the rooms that
//! came due.
//!
//! Everything here is pure: no clock is read, nothing is sent, and nothing is
//! stored. [`digest`] turns the rooms that came due into the plain text of one
//! message per enabled sink, and [`coalesce_window`] answers whether the
//! channel's minimum interval has elapsed.
//!
//! The body is a counts-only digest by default. It names each room by the safe
//! display label the attention projection produced, with the kind and the counts,
//! and carries no room id, sender id, event id, or message text. Message text
//! appears only as the prefix of a room, only for a sink and destination whose
//! stored consent matches, and only flattened to one line and capped at a
//! grapheme-cluster boundary, so one message cannot forge another room's line and
//! a long message cannot smuggle a page of text through a consent given for a
//! glance (`docs/architecture/state-machine.md`, "Local Notification Delivery").

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use koushi_state::{
    MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS, NotificationDeliverySettings, NotificationSinkSettings,
    RoomAttentionKind,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::executor::Instant;

use super::dwell::QualifyingRoom;

/// How many rooms one digest spells out before it ends with an `and N more` tail.
///
/// A large account produces one bounded message instead of a page of room lines,
/// and the tail keeps the count of the rooms the body does not name, so nothing
/// is dropped silently.
pub const MAX_NOTIFICATION_DIGEST_ROOMS: usize = 5;

/// Everything one sender needs to deliver one message, except the credential.
///
/// The credential stays in the credential store and is read by the caller when it
/// sends; it never enters this type or this module. Every field is non-secret, but
/// the body may carry message text when a prefix consent covers the sink, so the
/// `Debug` implementation redacts the addresses and the message text rather than
/// copying them into a log or a diagnostics export.
#[derive(Clone, Eq, PartialEq)]
pub struct NotificationDeliveryIntent {
    /// The sink this intent is for, naming the credential to read and the
    /// destination it belongs to.
    pub sink_id: String,
    /// Where the message goes: the mailbox for a mail sink.
    pub destination: String,
    /// The address the recipient sees as the sender.
    pub from_address: String,
    /// The name the recipient sees next to the address. Empty means the bare
    /// address, which is the sender's decision, not this module's.
    pub sender_name: String,
    /// The plain-text subject: the number of rooms that came due.
    pub subject: String,
    /// The plain-text body: one block per delivered room, plus an `and N more`
    /// tail when more rooms came due than the body names.
    pub body: String,
    /// The key that makes a retry of one window idempotent. It is derived from the
    /// sink id and the caller's window id, so a retry of the same window repeats
    /// the key and a later window never reuses it.
    pub idempotency_key: String,
}

/// `Debug` redacts the recipient, the sender address, the subject, and the body:
/// the addresses are personal identifiers and the body may carry message text.
/// The sink id and the idempotency key stay visible, because they name no person
/// and no message and are what a retry is correlated by.
impl fmt::Debug for NotificationDeliveryIntent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationDeliveryIntent")
            .field("sink_id", &self.sink_id)
            .field("destination", &"Recipient(..)")
            .field("from_address", &"Sender(..)")
            .field("sender_name", &"SenderName(..)")
            .field("subject", &"Subject(..)")
            .field("body", &"Body(..)")
            .field("idempotency_key", &self.idempotency_key)
            .finish()
    }
}

/// Build one delivery intent per enabled, usable sink, for the rooms that came
/// due in one window.
///
/// `due` is the set the dwell tracker released for this window, and `window_id`
/// names that window - a timestamp, a counter, or whatever the caller's coalescing
/// uses. An empty `due` returns no intents at all, so does a disabled channel, and
/// so does a settings value with no enabled, complete sink. This function does not
/// send, does not read a clock, and does not touch a credential.
///
/// [`NotificationDeliverySettings::enabled`] is the channel's own off switch, not a
/// filter: an app the user has not turned on produces nothing here, whatever the
/// caller passes as due.
///
/// The subject is the number of rooms that came due. The body names each room by
/// its safe display label, its kind, and its counts, in a deterministic order, and
/// carries no room id, sender id, event id, or message text. A room's
/// `plaintext_prefix` is prepended to that room's own block only when
/// [`NotificationDeliverySettings::prefix_is_allowed_for`] holds for this sink, its
/// destination, and its endpoint; it is flattened to a single line and capped at
/// `prefix_chars` grapheme clusters, never more than
/// [`MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS`]. Everything the caller receives is
/// plain text.
///
/// The idempotency key is the sink id and the window id, joined by `/` after a
/// `koushi-notification` prefix. A canonical sink id cannot contain `/`, so the
/// key is unambiguous, a retry inside one window repeats it, and a later window
/// never collides with it.
///
/// Which rooms qualify is the caller's decision, and this function must not
/// second-guess it: the enabled trigger kinds, the watched rooms and spaces (which
/// deliver even when their category is off), the window focus, and the dwell all
/// shape the qualifying set before it reaches here. A plain `Message` room has no
/// trigger flag of its own and a keyword hit is indistinguishable from a mention in
/// the attention projection, so the filtering belongs where the set is built, not
/// in the body.
pub fn digest(
    settings: &NotificationDeliverySettings,
    due: &[QualifyingRoom],
    window_id: &str,
) -> Vec<NotificationDeliveryIntent> {
    if !settings.enabled {
        return Vec::new();
    }
    let rooms = ordered_rooms(due);
    if rooms.is_empty() {
        return Vec::new();
    }
    let subject = digest_subject(rooms.len());

    let mut intents = Vec::new();
    for sink in &settings.sinks {
        if !sink_is_deliverable(sink) {
            continue;
        }
        // A consent is bound to one sink, destination, and endpoint, and it also
        // carries the option being on: `prefix_is_allowed_for` is false when
        // `prefix_chars` is zero.
        let prefix_chars =
            if settings.prefix_is_allowed_for(&sink.id, &sink.destination, &sink.endpoint()) {
                settings.prefix_chars
            } else {
                0
            };
        intents.push(NotificationDeliveryIntent {
            sink_id: sink.id.clone(),
            destination: sink.destination.clone(),
            from_address: sink.from_address.clone(),
            sender_name: sink.sender_name.clone(),
            subject: subject.clone(),
            body: digest_body(&rooms, prefix_chars),
            idempotency_key: idempotency_key(&sink.id, window_id),
        });
    }

    intents
}

/// Whether the channel's minimum interval between two deliveries has elapsed, so a
/// send may happen now.
///
/// This is the coalescing gate, not the dwell: the dwell holds one room until its
/// own attention state persists, while this interval keeps a busy account from
/// filling a mailbox with one message per room that comes due, and it is what a
/// retry of a transport or rate-limit failure honors. `last_delivery_at` is when the
/// channel last sent or last attempted a send, `None` when it never has.
///
/// A `None` timestamp, an interval of zero, and an interval that has elapsed all
/// return true. A timestamp in the future returns false, which fails closed on a
/// clock that moved backwards.
pub fn coalesce_window(
    settings: &NotificationDeliverySettings,
    last_delivery_at: Option<Instant>,
    now: Instant,
) -> bool {
    let Some(last_delivery_at) = last_delivery_at else {
        return true;
    };
    let interval = Duration::from_secs(settings.min_interval_secs);
    now.checked_duration_since(last_delivery_at)
        .is_some_and(|elapsed| elapsed >= interval)
}

/// Whether one sink carries everything a send needs. The settings loader already
/// drops an incomplete or duplicated sink in `canonicalize`, and this re-checks the
/// invariants that are observable from outside, so a hand-built settings value
/// cannot produce an intent with no recipient or no endpoint.
fn sink_is_deliverable(sink: &NotificationSinkSettings) -> bool {
    sink.enabled
        && !sink.id.trim().is_empty()
        && !sink.destination.trim().is_empty()
        && !sink.from_address.trim().is_empty()
        && !sink.endpoint().trim().is_empty()
}

/// Put the due rooms in the order the body renders them.
///
/// The order is the candidate order the attention projection already ranks by -
/// mention before direct message before message, then more mentions, then more
/// unread, then the display label - with the room id as a final tiebreak so the
/// order is total and never depends on the order of the input. A room id that
/// appears twice contributes one entry, the highest-ranked observation.
fn ordered_rooms(due: &[QualifyingRoom]) -> Vec<&QualifyingRoom> {
    let mut rooms: Vec<&QualifyingRoom> = due.iter().collect();
    rooms.sort_by(|left, right| {
        attention_kind_priority(right.kind)
            .cmp(&attention_kind_priority(left.kind))
            .then_with(|| right.mentions.cmp(&left.mentions))
            .then_with(|| right.unread.cmp(&left.unread))
            .then_with(|| left.display_label.cmp(&right.display_label))
            .then_with(|| left.room_id.cmp(&right.room_id))
    });
    let mut seen = BTreeSet::new();
    rooms.retain(|room| seen.insert(room.room_id.as_str()));
    rooms
}

/// Rank the attention kinds the way the attention projection ranks a candidate.
fn attention_kind_priority(kind: RoomAttentionKind) -> u8 {
    match kind {
        RoomAttentionKind::Mention => 3,
        RoomAttentionKind::Dm => 2,
        RoomAttentionKind::Message => 1,
    }
}

/// The plain-text subject of one digest: how many rooms came due, and never which
/// rooms they are.
fn digest_subject(room_count: usize) -> String {
    match room_count {
        1 => "1 room needs attention".to_owned(),
        count => format!("{count} rooms need attention"),
    }
}

/// Render the body of one digest, naming at most
/// [`MAX_NOTIFICATION_DIGEST_ROOMS`] rooms and keeping the rest as a counted tail.
///
/// `prefix_chars` is the grapheme cap for each room's plaintext prefix, and zero
/// means no prefix is rendered at all.
fn digest_body(rooms: &[&QualifyingRoom], prefix_chars: u32) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for room in rooms.iter().take(MAX_NOTIFICATION_DIGEST_ROOMS) {
        let mut block = String::new();
        if let Some(prefix) = room
            .plaintext_prefix
            .as_deref()
            .and_then(|text| bounded_prefix(text, prefix_chars))
        {
            block.push_str(&prefix);
            block.push('\n');
        }
        block.push_str(&room_line(room));
        blocks.push(block);
    }

    let remaining = rooms.len().saturating_sub(MAX_NOTIFICATION_DIGEST_ROOMS);
    if remaining > 0 {
        blocks.push(format!("and {remaining} more"));
    }

    blocks.join("\n\n")
}

/// One room's digest line: the kind, the safe display label, and the counts.
fn room_line(room: &QualifyingRoom) -> String {
    let label = safe_label(room);
    let headline = match room.kind {
        RoomAttentionKind::Mention => format!("Mention in {label}"),
        RoomAttentionKind::Dm => format!("Direct message in {label}"),
        RoomAttentionKind::Message => format!("Message in {label}"),
    };
    let counts = room_counts(room.mentions, room.unread);
    if counts.is_empty() {
        headline
    } else {
        format!("{headline}: {counts}")
    }
}

/// A room's mention and unread counts as prose, with a zero count omitted. The
/// mention count is shown for every kind, because a direct message can carry a
/// mention too.
fn room_counts(mentions: u32, unread: u32) -> String {
    let mut parts = Vec::new();
    if mentions > 0 {
        parts.push(format_count(mentions, "mention", "mentions"));
    }
    if unread > 0 {
        parts.push(format_count(unread, "unread", "unread"));
    }
    parts.join(", ")
}

/// One count with its noun, singular at one.
fn format_count(count: u32, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// The room's display label as the body renders it: one line, trimmed, and never
/// empty.
///
/// The attention projection already substitutes `Room` for an empty label; that
/// fallback is repeated here so a hand-built observation cannot render a nameless
/// line.
fn safe_label(room: &QualifyingRoom) -> String {
    let label = single_line(&room.display_label);
    let label = label.trim();
    if label.is_empty() {
        "Room".to_owned()
    } else {
        label.to_owned()
    }
}

/// The plaintext prefix as the body renders it: one line, trimmed, capped at
/// `prefix_chars` grapheme clusters, and `None` when nothing is left or no prefix
/// is allowed.
///
/// Newlines and other control characters are replaced by spaces before the cap, so
/// message text cannot start a line that reads as another room's.
fn bounded_prefix(text: &str, prefix_chars: u32) -> Option<String> {
    let cap = prefix_chars.min(MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS);
    if cap == 0 {
        return None;
    }
    let flattened = single_line(text);
    let trimmed = capped_graphemes(&flattened, cap).trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// The longest prefix of `text` that is at most `max_graphemes` grapheme clusters.
///
/// A grapheme cluster is what a reader calls one character, so a cap never splits a
/// combining mark from its base, an emoji from its joiner, or a flag from its
/// selector. The cut is always on a byte boundary of `text`.
fn capped_graphemes(text: &str, max_graphemes: u32) -> &str {
    match text.grapheme_indices(true).nth(max_graphemes as usize) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

/// Replace every control character and both Unicode line separators with a space,
/// so text that is rendered as one line cannot invent a line break.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// The key that makes one window's send to one sink idempotent.
///
/// A canonical sink id is lowercase ASCII letters, digits, `-`, and `_`, so it
/// never contains `/` and the two parts cannot be read as a different pair.
fn idempotency_key(sink_id: &str, window_id: &str) -> String {
    format!("koushi-notification/{sink_id}/{window_id}")
}

/// The subject a test send carries.
///
/// English, like the digest's own wording and for the same reason: this text never
/// renders inside the app, it is what the recipient's mail client shows, and the
/// message catalog owns what the app says, not what this device puts on the wire.
pub const TEST_DELIVERY_SUBJECT: &str = "Koushi test notification";

/// The body a test send carries: what it is and what it proves, and nothing else.
///
/// It names no room, event, sender, or message, because a test send happens before
/// the user has trusted the channel with any of that
/// (`docs/architecture/state-machine.md`, "Local Notification Delivery").
pub const TEST_DELIVERY_BODY: &str = "This is a test notification from Koushi.\n\
     If you can read it, this device can reach this delivery sink. No room, message, or \
     account data is included in a test send.";

/// Build the one canned intent a test send delivers through one sink.
///
/// The intent goes through the same sink path, the same credential read, and the same typed
/// outcome projection as a real delivery, and it carries no attention state at all. It is
/// deliberately kept out of the dispatch window: the caller records its outcome without touching
/// the coalescing window, so testing a sink can neither suppress nor delay a real notification.
///
/// Neither the channel's own switch nor the sink's is required. A test send happens while the
/// user is still setting the sink up, and asking them to turn the channel on before they can
/// find out whether it works would be asking them to trust it first. The sink itself is always
/// complete - the settings loader drops one that is not - so the request it builds is always a
/// request a real send would have built.
pub fn test_delivery_intent(
    sink: &NotificationSinkSettings,
    window_id: &str,
) -> NotificationDeliveryIntent {
    NotificationDeliveryIntent {
        sink_id: sink.id.clone(),
        destination: sink.destination.clone(),
        from_address: sink.from_address.clone(),
        sender_name: sink.sender_name.clone(),
        subject: TEST_DELIVERY_SUBJECT.to_owned(),
        body: TEST_DELIVERY_BODY.to_owned(),
        idempotency_key: idempotency_key(&sink.id, window_id),
    }
}
