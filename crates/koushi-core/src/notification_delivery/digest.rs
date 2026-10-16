//! The message one destination receives for the rooms it holds (#1427, #1460).
//!
//! Everything here is pure: no clock is read, nothing is sent, and nothing is stored.
//! [`render_intent`] turns the rooms one lane holds into one message for that lane's
//! destination, and [`coalesce_window`] answers whether the destination's minimum
//! interval has elapsed. The caller renders at send time, from the current observation,
//! so a held or retried message never carries a state the badge no longer shows (#1454).
//!
//! A room is named by the safe display label the attention projection produced, its
//! first parent space, its kind, and its counts, and never by a room, event, sender, or
//! transaction id. Message text appears only when the destination's effective content
//! mode carries it (the caller leaves [`QualifyingRoom::message_text`] empty otherwise),
//! bounded by the mode and by the provider's per-room budget; text cut by either limit is
//! marked `[truncated]` and is never called the entire message. Text is rendered as
//! quoted lines so it cannot forge another room's line, and a ServerChan body has its
//! Markdown punctuation escaped so it cannot inject a link or an image
//! (`docs/architecture/state-machine.md`, "Local Notification Delivery").

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use koushi_state::{
    NotificationDeliveryContent, NotificationSinkConfig, NotificationSinkSettings,
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

/// The longest subject a mail digest carries, in grapheme clusters.
pub const MAX_NOTIFICATION_SUBJECT_GRAPHEMES: usize = 200;

/// The longest ServerChan `title`, in grapheme clusters. The service caps the title at
/// 32 characters, and the WeChat card may show nothing else.
pub const MAX_SERVERCHAN_TITLE_GRAPHEMES: usize = 32;

/// The marker appended to text that a mode or a provider budget cut.
pub const NOTIFICATION_TEXT_TRUNCATED_MARKER: &str = "[truncated]";

/// Everything one sender needs to deliver one message, except the credential.
///
/// The credential stays in the credential store and is read by the caller when it
/// sends; it never enters this type or this module. Every field is non-secret, but
/// the body may carry message text when the destination's consent covers it, so the
/// `Debug` implementation redacts the addresses and the message text rather than
/// copying them into a log or a diagnostics export.
#[derive(Clone, Eq, PartialEq)]
pub struct NotificationDeliveryIntent {
    /// The sink this intent is for, naming the credential to read and the
    /// destination it belongs to.
    pub sink_id: String,
    /// Where the message goes: the mailbox for a mail sink, empty for ServerChan,
    /// whose SendKey names the recipient.
    pub destination: String,
    /// The address the recipient sees as the sender.
    pub from_address: String,
    /// The name the recipient sees next to the address. Empty means the bare
    /// address, which is the sender's decision, not this module's.
    pub sender_name: String,
    /// The plain-text subject, or the ServerChan `title`: always one line and names
    /// only.
    pub subject: String,
    /// The body: plain text for mail, Markdown for ServerChan's `desp`.
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

/// How the body is written for one destination.
#[derive(Clone, Copy, Eq, PartialEq)]
enum BodyFormat {
    /// Plain text, for a mail provider or an SMTP relay.
    PlainText,
    /// ServerChan `desp`: Markdown, with every piece of user text escaped.
    Markdown,
}

/// Render the one message a destination receives for `rooms`, in window `window_id`.
///
/// `rooms` is the lane's current observation of the rooms it is about to send, already
/// filtered by the destination's targets and focus rule; this function does not
/// second-guess that set. It returns `None` when no room is left or when the destination
/// lacks a field a send needs, so a hand-built settings value cannot produce an intent
/// with no recipient. It does not send, read a clock, or touch a credential.
///
/// The idempotency key is the sink id and the window id, joined by `/` after a
/// `koushi-notification` prefix. A canonical sink id cannot contain `/`, so the key is
/// unambiguous, a retry inside one window repeats it, and a later window never collides
/// with it.
pub fn render_intent(
    sink: &NotificationSinkSettings,
    rooms: &[QualifyingRoom],
    window_id: &str,
) -> Option<NotificationDeliveryIntent> {
    if !sink_is_deliverable(sink) {
        return None;
    }
    let rooms = ordered_rooms(rooms);
    let first = rooms.first()?;
    let format = match sink.config {
        NotificationSinkConfig::ServerChan { .. } => BodyFormat::Markdown,
        NotificationSinkConfig::Resend { .. } | NotificationSinkConfig::Smtp { .. } => {
            BodyFormat::PlainText
        }
    };
    let subject_cap = match format {
        BodyFormat::Markdown => MAX_SERVERCHAN_TITLE_GRAPHEMES,
        BodyFormat::PlainText => MAX_NOTIFICATION_SUBJECT_GRAPHEMES,
    };
    let text_cap = text_cap_graphemes(sink);
    Some(NotificationDeliveryIntent {
        sink_id: sink.id.clone(),
        destination: sink.destination.clone(),
        from_address: sink.from_address.clone(),
        sender_name: sink.sender_name.clone(),
        subject: digest_subject(first, rooms.len(), subject_cap),
        body: digest_body(&rooms, text_cap, format),
        idempotency_key: idempotency_key(&sink.id, window_id),
    })
}

/// Whether the destination's minimum interval between two attempts has elapsed, so a
/// send may happen now.
///
/// This is the coalescing gate, not the dwell: the dwell holds one room until its own
/// attention state persists, while this interval keeps a busy account from filling an
/// inbox with one message per room that comes due. `last_attempt_at` is when the lane
/// last attempted a send, `None` when it never has.
///
/// A `None` timestamp, an interval of zero, and an interval that has elapsed all return
/// true. A timestamp in the future returns false, which fails closed on a clock that
/// moved backwards.
pub fn coalesce_window(
    min_interval: Duration,
    last_attempt_at: Option<Instant>,
    now: Instant,
) -> bool {
    let Some(last_attempt_at) = last_attempt_at else {
        return true;
    };
    now.checked_duration_since(last_attempt_at)
        .is_some_and(|elapsed| elapsed >= min_interval)
}

/// The per-room text cap for this destination: the content mode's own cap, never more
/// than the provider budget. Zero means no text is rendered.
fn text_cap_graphemes(sink: &NotificationSinkSettings) -> usize {
    let budget = sink.text_budget_graphemes();
    match sink.effective_content() {
        NotificationDeliveryContent::NamesOnly => 0,
        NotificationDeliveryContent::Prefix { graphemes } => (graphemes as usize).min(budget),
        NotificationDeliveryContent::Full => budget,
    }
}

/// Whether one sink carries everything a send needs. The settings loader already
/// drops an incomplete or duplicated sink in `canonicalize`, and this re-checks the
/// invariants that are observable from outside, so a hand-built settings value
/// cannot produce an intent with no recipient or no endpoint.
fn sink_is_deliverable(sink: &NotificationSinkSettings) -> bool {
    if sink.id.trim().is_empty() || sink.endpoint().trim().is_empty() {
        return false;
    }
    match sink.config {
        NotificationSinkConfig::ServerChan { .. } => true,
        NotificationSinkConfig::Resend { .. } | NotificationSinkConfig::Smtp { .. } => {
            !sink.destination.trim().is_empty() && !sink.from_address.trim().is_empty()
        }
    }
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

/// The one-line, names-only subject (or ServerChan title): the first room's headline
/// and how many more rooms the message covers.
fn digest_subject(first: &QualifyingRoom, room_count: usize, cap: usize) -> String {
    let headline = room_headline(first);
    let subject = match room_count.saturating_sub(1) {
        0 => headline,
        1 => format!("{headline} and 1 more room"),
        more => format!("{headline} and {more} more rooms"),
    };
    ellipsize(&subject, cap)
}

/// Render the body of one digest, naming at most [`MAX_NOTIFICATION_DIGEST_ROOMS`]
/// rooms and keeping the rest as a counted tail.
fn digest_body(rooms: &[&QualifyingRoom], text_cap: usize, format: BodyFormat) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for room in rooms.iter().take(MAX_NOTIFICATION_DIGEST_ROOMS) {
        let mut block = match format {
            BodyFormat::PlainText => room_line(room),
            BodyFormat::Markdown => escape_markdown(&room_line(room)),
        };
        if let Some(text) = room_text(room, text_cap, format) {
            block.push('\n');
            block.push_str(&text);
        }
        blocks.push(block);
    }

    let remaining = rooms.len().saturating_sub(MAX_NOTIFICATION_DIGEST_ROOMS);
    if remaining > 0 {
        blocks.push(format!("and {remaining} more"));
    }

    blocks.join("\n\n")
}

/// One room's headline: the kind, the safe display label, and the first parent space.
fn room_headline(room: &QualifyingRoom) -> String {
    let label = safe_label(&room.display_label).unwrap_or_else(|| "Room".to_owned());
    let place = match room.space_labels.iter().find_map(|space| safe_label(space)) {
        Some(space) => format!("{label} · {space}"),
        None => label,
    };
    match room.kind {
        RoomAttentionKind::Mention => format!("Mention in {place}"),
        RoomAttentionKind::Dm => format!("Direct message in {place}"),
        RoomAttentionKind::Message => format!("Message in {place}"),
    }
}

/// One room's digest line: the headline and the counts.
fn room_line(room: &QualifyingRoom) -> String {
    let headline = room_headline(room);
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

/// A label as a digest renders it: one line, trimmed, `None` when nothing is left.
fn safe_label(label: &str) -> Option<String> {
    let label = single_line(label);
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_owned())
}

/// The room's message text as a quoted block, cut at `cap` grapheme clusters and marked
/// `[truncated]` when the cap or the source bound cut it. `None` when the room carries
/// no text, the cap is zero, or nothing printable is left.
///
/// Every line of the text starts with `> `, so message text can never start a line that
/// reads as another room's headline.
fn room_text(room: &QualifyingRoom, cap: usize, format: BodyFormat) -> Option<String> {
    if cap == 0 {
        return None;
    }
    let source = room.message_text.as_ref()?;
    let normalized = safe_lines(&source.text);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    let capped = capped_graphemes(trimmed, cap as u32);
    let truncated = source.truncated_at_source || capped.len() < trimmed.len();
    let mut lines: Vec<String> = capped
        .trim_end()
        .lines()
        .map(|line| match format {
            BodyFormat::PlainText => format!("> {line}"),
            BodyFormat::Markdown => format!("> {}  ", escape_markdown(line)),
        })
        .collect();
    if truncated {
        lines.push(format!("> {NOTIFICATION_TEXT_TRUNCATED_MARKER}"));
    }
    Some(lines.join("\n"))
}

/// Text as a sequence of safe lines: a line break stays a line break, every other control
/// character and both Unicode separators become a space or a break, so nothing renders
/// as a hidden control.
fn safe_lines(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\n' | '\u{2028}' | '\u{2029}' => '\n',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect()
}

/// Escape every ASCII Markdown punctuation character, so user text in a ServerChan
/// `desp` is rendered literally and cannot open a link, an image, or raw HTML.
fn escape_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// One line of at most `cap` grapheme clusters, ending in `…` when it was cut.
fn ellipsize(text: &str, cap: usize) -> String {
    let line = single_line(text);
    let line = line.trim();
    let capped = capped_graphemes(line, cap.saturating_sub(1) as u32);
    if line.graphemes(true).count() <= cap {
        line.to_owned()
    } else {
        format!("{}…", capped.trim_end())
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
pub(crate) fn idempotency_key(sink_id: &str, window_id: &str) -> String {
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
/// No target is required. A test send happens while the user is still setting the destination
/// up, and asking them to choose what to send before they can find out whether it works would
/// be asking them to trust it first. The sink itself is always complete - the settings loader
/// drops one that is not - so the request it builds is always a request a real send would
/// have built.
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
