//! Device-local notification delivery settings (#1427).
//!
//! The channel that lets this device send its own notification - to a mail provider
//! such as Resend, or through an SMTP relay such as QQ Mail, Gmail, or iCloud - is
//! app-scoped: it never asks the Matrix account for a destination, so it works on a
//! homeserver that supports neither email 3PIDs nor pushers. Every value here is
//! non-secret. A sink's credential lives in the credential store, keyed by the sink
//! id, and is reported to the product only as a configured flag
//! (state-machine.md, "Local Notification Delivery").

use serde::{Deserialize, Serialize};

/// How long a room must stay in a qualifying attention state before this device
/// delivers a notification about it. Five minutes by default: long enough that a
/// conversation the user is already reading is never mailed.
pub const DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS: u64 = 300;
/// The shortest gap between two deliveries of the same channel. Fifteen minutes by
/// default, so a busy group cannot fill a mailbox.
pub const DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS: u64 = 900;
/// A day is the longest dwell that still means "while you were away"; anything
/// longer would deliver a state the user has long since seen.
pub const MAX_NOTIFICATION_DELIVERY_DWELL_SECS: u64 = 86_400;
pub const MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS: u64 = 86_400;
/// The plaintext prefix is capped so a long message cannot smuggle a page of text
/// through a consent the user gave for a glance.
pub const MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS: u32 = 200;
pub const DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS: u32 = 80;
/// Sinks are a short list the user configures by hand; the cap keeps a corrupted or
/// hostile settings file from turning one settings load into unbounded work.
pub const MAX_NOTIFICATION_DELIVERY_SINKS: usize = 8;
/// The watch list is capped as well: a candidate set is built on every attention
/// update, so the membership lookup has to stay bounded.
pub const MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS: usize = 64;
pub const MAX_NOTIFICATION_DELIVERY_FIELD_CHARS: usize = 320;
const MAX_NOTIFICATION_SINK_ID_BYTES: usize = 64;

/// Default endpoint of the Resend sink, matching `koushi_notify::RESEND_ENDPOINT`.
/// It is repeated here because `koushi-state` is a data crate with no dependency on
/// the senders; the core compares the two when it builds a delivery.
pub const DEFAULT_RESEND_ENDPOINT: &str = "https://api.resend.com/emails";

/// Whether this device may deliver a notification at all, and what for.
///
/// The channel is OFF by default: an unconfigured app must not start sending mail.
/// The three triggers default to ON so that turning the channel on is one decision,
/// not four.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationDeliverySettings {
    pub enabled: bool,
    /// Deliver for a direct message.
    pub direct_messages: bool,
    /// Deliver for a mention, which includes a keyword hit: keywords are Matrix
    /// `kind: content` push rules and rank as mention-kind in the attention
    /// projection, so they need no separate delivery path.
    pub mentions: bool,
    /// Deliver for a keyword the user defined.
    pub keywords: bool,
    /// Hold delivery while this app is focused, the way the OS suppresses a banner
    /// the user is already looking at. Recommended, and the default.
    pub only_when_unfocused: bool,
    pub dwell_secs: u64,
    pub min_interval_secs: u64,
    /// Characters of the decrypted message body to prepend in plaintext. Applied only
    /// when `prefix_consent` matches the sink it would be sent through.
    pub prefix_chars: u32,
    pub prefix_consent: Option<NotificationDeliveryPrefixConsent>,
    /// Rooms and spaces the user named, delivered whatever the three categories above
    /// say. A watched space covers every joined room that is a child of it, resolved
    /// from the room list when the candidate set is built, so a room added to the space
    /// later is covered without editing this list.
    pub watched: Vec<NotificationDeliveryTarget>,
    pub sinks: Vec<NotificationSinkSettings>,
}

impl Default for NotificationDeliverySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            direct_messages: true,
            mentions: true,
            keywords: true,
            only_when_unfocused: true,
            dwell_secs: DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS,
            min_interval_secs: DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS,
            prefix_chars: DEFAULT_NOTIFICATION_DELIVERY_PREFIX_CHARS,
            prefix_consent: None,
            watched: Vec::new(),
            sinks: Vec::new(),
        }
    }
}

impl NotificationDeliverySettings {
    pub fn sink(&self, id: &str) -> Option<&NotificationSinkSettings> {
        self.sinks.iter().find(|sink| sink.id == id)
    }

    /// Whether at least one trigger is on, so the product can say why nothing is
    /// delivered instead of looking broken.
    pub fn has_any_trigger(&self) -> bool {
        self.direct_messages || self.mentions || self.keywords
    }

    /// Whether any configured sink could deliver: enabled, complete, and with a
    /// credential the core reported as present.
    pub fn has_enabled_sink(&self) -> bool {
        self.sinks.iter().any(|sink| sink.enabled)
    }

    /// Whether the stored consent covers this exact sink and destination.
    ///
    /// A change to the sink, its endpoint, or the destination is a change to what
    /// leaves the device, so it invalidates the consent rather than inheriting it.
    pub fn prefix_is_allowed_for(&self, sink_id: &str, destination: &str, endpoint: &str) -> bool {
        self.prefix_chars > 0
            && self.prefix_consent.as_ref().is_some_and(|consent| {
                consent.sink_id == sink_id
                    && consent.destination == destination
                    && consent.endpoint == endpoint
            })
    }

    /// Whether this exact room is named in the watch list. A watched space is resolved
    /// by the caller, which knows the space's children.
    pub fn watches_room(&self, room_id: &str) -> bool {
        self.watched
            .iter()
            .any(|target| matches!(target, NotificationDeliveryTarget::Room { room_id: watched } if watched == room_id))
    }

    /// Whether this space is named in the watch list.
    pub fn watches_space(&self, space_id: &str) -> bool {
        self.watched
            .iter()
            .any(|target| matches!(target, NotificationDeliveryTarget::Space { space_id: watched } if watched == space_id))
    }

    /// Apply what this device must never depend on a well-formed settings file for:
    /// bounded numbers, bounded strings, unique sink ids and watch targets, and a
    /// consent that still matches a sink. Called on every load and every patch.
    pub fn canonicalize(&mut self) {
        self.dwell_secs = self.dwell_secs.min(MAX_NOTIFICATION_DELIVERY_DWELL_SECS);
        self.min_interval_secs = self
            .min_interval_secs
            .min(MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS);
        self.prefix_chars = self
            .prefix_chars
            .min(MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS);

        let mut canonical: Vec<NotificationSinkSettings> = Vec::new();
        for mut sink in std::mem::take(&mut self.sinks) {
            sink.canonicalize();
            if !sink.is_usable() || canonical.iter().any(|existing| existing.id == sink.id) {
                continue;
            }
            canonical.push(sink);
            if canonical.len() == MAX_NOTIFICATION_DELIVERY_SINKS {
                break;
            }
        }
        self.sinks = canonical;

        let mut watched: Vec<NotificationDeliveryTarget> = Vec::new();
        for mut target in std::mem::take(&mut self.watched) {
            target.canonicalize();
            if !target.is_usable()
                || watched
                    .iter()
                    .any(|existing| existing.kind_and_id() == target.kind_and_id())
            {
                continue;
            }
            watched.push(target);
            if watched.len() == MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS {
                break;
            }
        }
        self.watched = watched;

        let consent_still_matches = self.prefix_consent.as_ref().is_some_and(|consent| {
            self.sinks.iter().any(|sink| {
                sink.id == consent.sink_id
                    && sink.destination == consent.destination
                    && sink.endpoint() == consent.endpoint
            })
        });
        if !consent_still_matches {
            self.prefix_consent = None;
        }
    }
}

/// Deserialize hook for the settings file. A delivery block read from disk is bounded,
/// deduplicated, and de-consented before any product code can see it, so a hand-edited
/// or older file cannot widen what leaves the device.
pub(crate) fn deserialize_notification_delivery<'de, D>(
    deserializer: D,
) -> Result<Box<NotificationDeliverySettings>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut settings = NotificationDeliverySettings::deserialize(deserializer)?;
    settings.canonicalize();
    Ok(Box::new(settings))
}

/// A room or a space the user named, delivered whatever the trigger categories say.
///
/// Only the id is stored. A label would be a copy of a name the room list already owns,
/// and it would go stale the moment the room is renamed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationDeliveryTarget {
    Room { room_id: String },
    Space { space_id: String },
}

impl NotificationDeliveryTarget {
    /// The Matrix id this target names.
    pub fn id(&self) -> &str {
        match self {
            Self::Room { room_id } => room_id,
            Self::Space { space_id } => space_id,
        }
    }

    fn kind_and_id(&self) -> (&'static str, &str) {
        match self {
            Self::Room { room_id } => ("room", room_id),
            Self::Space { space_id } => ("space", space_id),
        }
    }

    fn canonicalize(&mut self) {
        let id = trimmed_field(self.id());
        match self {
            Self::Room { room_id } => *room_id = id,
            Self::Space { space_id } => *space_id = id,
        }
    }

    /// A target has to name a room or space id (`!`) or an alias (`#`), bounded, and free
    /// of control characters. Anything else is dropped here rather than resolved later.
    fn is_usable(&self) -> bool {
        let id = self.id();
        matches!(id.chars().next(), Some('!' | '#'))
            && id.chars().count() > 1
            && id.len() <= MAX_NOTIFICATION_DELIVERY_FIELD_CHARS
            && !id.chars().any(char::is_control)
    }
}

/// One configured way out of the device.
///
/// The id names the credential in the credential store and is the only identity a
/// sink has; two sinks never share one.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationSinkSettings {
    pub id: String,
    pub enabled: bool,
    /// Where the message goes: the mailbox for a mail sink.
    pub destination: String,
    pub from_address: String,
    /// The name the recipient sees. Empty falls back to the bare address.
    pub sender_name: String,
    pub config: NotificationSinkConfig,
}

impl NotificationSinkSettings {
    /// The non-secret string a prefix consent binds to, so that editing the endpoint
    /// asks for the consent again.
    pub fn endpoint(&self) -> String {
        match &self.config {
            NotificationSinkConfig::Resend { endpoint } => endpoint.clone(),
            NotificationSinkConfig::Smtp { host, port, .. } => format!("{host}:{port}"),
        }
    }

    fn canonicalize(&mut self) {
        self.id = self.id.trim().to_ascii_lowercase();
        self.destination = trimmed_field(&self.destination);
        self.from_address = trimmed_field(&self.from_address);
        self.sender_name = trimmed_field(&self.sender_name);
        match &mut self.config {
            NotificationSinkConfig::Resend { endpoint } => {
                *endpoint = trimmed_field(endpoint);
                if endpoint.is_empty() {
                    *endpoint = DEFAULT_RESEND_ENDPOINT.to_owned();
                }
            }
            NotificationSinkConfig::Smtp {
                preset,
                host,
                port,
                username,
                ..
            } => {
                *preset = preset
                    .as_ref()
                    .map(|value| value.trim().to_ascii_lowercase());
                if preset.as_deref() == Some("") {
                    *preset = None;
                }
                *host = trimmed_field(host);
                *port = (*port).max(1);
                *username = trimmed_field(username);
            }
        }
    }

    /// Whether the sink carries enough to be attempted at all. A sink that fails this
    /// is dropped rather than shown as configured-but-broken.
    fn is_usable(&self) -> bool {
        is_valid_notification_sink_id(&self.id)
            && !self.destination.is_empty()
            && !self.from_address.is_empty()
            && match &self.config {
                NotificationSinkConfig::Resend { endpoint } => !endpoint.is_empty(),
                NotificationSinkConfig::Smtp { host, port, .. } => !host.is_empty() && *port > 0,
            }
    }
}

/// The transport a sink speaks. Tagged by `kind` so a settings file and the frontend
/// DTO carry the same shape the core dispatches on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationSinkConfig {
    /// A provider HTTPS API.
    Resend { endpoint: String },
    /// Any SMTP relay, including the mailbox presets. The credential is the mailbox
    /// authorization code or app password, never the account password.
    Smtp {
        preset: Option<String>,
        host: String,
        port: u16,
        tls: SmtpTls,
        username: String,
    },
}

impl Default for NotificationSinkConfig {
    fn default() -> Self {
        Self::Resend {
            endpoint: DEFAULT_RESEND_ENDPOINT.to_owned(),
        }
    }
}

/// How the SMTP conversation secures itself. Mailbox presets pick one; a custom relay
/// chooses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SmtpTls {
    /// TLS from the first byte, the usual port 465 shape.
    #[default]
    Implicit,
    /// Plaintext greeting, then `STARTTLS` before the credential, the port 587 shape.
    StartTls,
}

/// The user's acceptance of sending message text off the device unencrypted, for one
/// sink and one destination.
///
/// Stored separately from the character count it enables, so a change to the sink or
/// the destination drops the consent instead of carrying it to a different recipient.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliveryPrefixConsent {
    pub sink_id: String,
    pub destination: String,
    /// The sink's non-secret endpoint at the moment of consent; see
    /// [`NotificationSinkSettings::endpoint`].
    pub endpoint: String,
    /// When the user accepted, in milliseconds since the Unix epoch, so a settings
    /// reader can say how old the acceptance is.
    pub accepted_at_ms: i64,
}

/// The charset a sink id may use.
///
/// `koushi_key::NotificationSinkSecretId::parse` is the owner of this rule, and the
/// core re-validates every id through it before the id can become a credential key.
/// This crate has no dependency on the key crate, so the alphabet is repeated here to
/// drop a sink that could never be keyed; a drift can therefore refuse a sink, never
/// admit one the credential store would reject.
fn is_valid_notification_sink_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_NOTIFICATION_SINK_ID_BYTES
        && id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
}

fn trimmed_field(value: &str) -> String {
    let mut trimmed = value.trim();
    if trimmed.chars().count() > MAX_NOTIFICATION_DELIVERY_FIELD_CHARS {
        trimmed = trimmed
            .char_indices()
            .nth(MAX_NOTIFICATION_DELIVERY_FIELD_CHARS)
            .map(|(index, _)| &trimmed[..index])
            .unwrap_or(trimmed);
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests;
