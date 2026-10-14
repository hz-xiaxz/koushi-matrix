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

    /// Apply what this device must never depend on a well-formed settings file for:
    /// bounded numbers, bounded strings, unique sink ids, and a consent that still
    /// matches a sink. Called on every load and every patch.
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
) -> Result<NotificationDeliverySettings, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut settings = NotificationDeliverySettings::deserialize(deserializer)?;
    settings.canonicalize();
    Ok(settings)
}

/// One configured way out of the device.
///
/// The id names the credential in the credential store and is the only identity a
/// sink has; two sinks never share one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

impl Default for NotificationSinkSettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: false,
            destination: String::new(),
            from_address: String::new(),
            sender_name: String::new(),
            config: NotificationSinkConfig::default(),
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SmtpTls {
    /// TLS from the first byte, the usual port 465 shape.
    Implicit,
    /// Plaintext greeting, then `STARTTLS` before the credential, the port 587 shape.
    StartTls,
}

impl Default for SmtpTls {
    fn default() -> Self {
        Self::Implicit
    }
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
mod tests {
    use super::*;

    fn sink(id: &str, destination: &str) -> NotificationSinkSettings {
        NotificationSinkSettings {
            id: id.to_owned(),
            enabled: true,
            destination: destination.to_owned(),
            from_address: "koushi@example.com".to_owned(),
            sender_name: "Koushi".to_owned(),
            config: NotificationSinkConfig::Resend {
                endpoint: DEFAULT_RESEND_ENDPOINT.to_owned(),
            },
        }
    }

    #[test]
    fn the_channel_is_off_and_preview_free_until_the_user_acts() {
        let settings = NotificationDeliverySettings::default();
        assert!(!settings.enabled);
        assert!(settings.sinks.is_empty());
        assert_eq!(settings.prefix_consent, None);
        assert!(settings.prefix_chars > 0);
        assert!(
            !settings.prefix_is_allowed_for("home", "me@example.com", DEFAULT_RESEND_ENDPOINT),
            "a character count without a consent must never allow message text out"
        );
    }

    #[test]
    fn canonicalize_bounds_numbers_fields_and_sinks() {
        let mut settings = NotificationDeliverySettings {
            dwell_secs: u64::MAX,
            min_interval_secs: u64::MAX,
            prefix_chars: u32::MAX,
            sinks: vec![
                sink("Home", " me@example.com "),
                sink("home", "other@example.com"),
                sink("bad id", "me@example.com"),
                sink("", "me@example.com"),
                sink("no-destination", "  "),
                sink("smtp", "me@example.com"),
                sink("s7", "me@example.com"),
                sink("s8", "me@example.com"),
                sink("s9", "me@example.com"),
                sink("s10", "me@example.com"),
                sink("s11", "me@example.com"),
                sink("s12", "me@example.com"),
                sink("s13", "me@example.com"),
            ],
            ..NotificationDeliverySettings::default()
        };
        settings.canonicalize();

        assert_eq!(settings.dwell_secs, MAX_NOTIFICATION_DELIVERY_DWELL_SECS);
        assert_eq!(
            settings.min_interval_secs,
            MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS
        );
        assert_eq!(
            settings.prefix_chars,
            MAX_NOTIFICATION_DELIVERY_PREFIX_CHARS
        );
        assert_eq!(settings.sinks.len(), MAX_NOTIFICATION_DELIVERY_SINKS);
        // The first mention of an id wins, and its fields are trimmed.
        assert_eq!(settings.sinks[0].id, "home");
        assert_eq!(settings.sinks[0].destination, "me@example.com");
        assert!(!settings.sinks.iter().any(|sink| sink.id == "bad id"));
        assert!(
            !settings
                .sinks
                .iter()
                .any(|sink| sink.id == "no-destination")
        );
        // The cap keeps the sinks the user configured first.
        assert!(settings.sinks.iter().any(|sink| sink.id == "s12"));
        assert!(!settings.sinks.iter().any(|sink| sink.id == "s13"));
    }

    #[test]
    fn a_consent_dies_with_the_sink_destination_or_endpoint_it_was_given_for() {
        let mut settings = NotificationDeliverySettings {
            sinks: vec![sink("home", "me@example.com")],
            prefix_consent: Some(NotificationDeliveryPrefixConsent {
                sink_id: "home".to_owned(),
                destination: "me@example.com".to_owned(),
                endpoint: DEFAULT_RESEND_ENDPOINT.to_owned(),
                accepted_at_ms: 1,
            }),
            ..NotificationDeliverySettings::default()
        };
        settings.canonicalize();
        assert!(settings.prefix_is_allowed_for("home", "me@example.com", DEFAULT_RESEND_ENDPOINT));

        // Changing the destination drops the consent rather than carrying it over.
        settings.sinks[0].destination = "other@example.com".to_owned();
        settings.canonicalize();
        assert_eq!(settings.prefix_consent, None);
        assert!(!settings.prefix_is_allowed_for(
            "home",
            "other@example.com",
            DEFAULT_RESEND_ENDPOINT
        ));

        // So does removing the sink entirely.
        settings.prefix_consent = Some(NotificationDeliveryPrefixConsent {
            sink_id: "home".to_owned(),
            destination: "other@example.com".to_owned(),
            endpoint: DEFAULT_RESEND_ENDPOINT.to_owned(),
            accepted_at_ms: 2,
        });
        settings.sinks.clear();
        settings.canonicalize();
        assert_eq!(settings.prefix_consent, None);
    }

    #[test]
    fn the_sink_id_charset_is_the_credential_key_charset() {
        assert!(is_valid_notification_sink_id("qq-mail"));
        assert!(is_valid_notification_sink_id("work_2"));
        assert!(!is_valid_notification_sink_id("Home"));
        assert!(!is_valid_notification_sink_id("me@example.com"));
        assert!(!is_valid_notification_sink_id("@user:example.org"));
        assert!(!is_valid_notification_sink_id("re-ABC_key"));
        assert!(!is_valid_notification_sink_id("sink id"));
        assert!(!is_valid_notification_sink_id(&"a".repeat(65)));
        assert!(!is_valid_notification_sink_id(""));
    }

    #[test]
    fn an_smtp_sink_binds_its_consent_to_host_and_port() {
        let mut settings = NotificationDeliverySettings::default();
        let mut smtp = sink("qq", "me@qq.com");
        smtp.config = NotificationSinkConfig::Smtp {
            preset: Some("qq-mail".to_owned()),
            host: "smtp.qq.com".to_owned(),
            port: 465,
            tls: SmtpTls::Implicit,
            username: "me@qq.com".to_owned(),
        };
        assert_eq!(smtp.endpoint(), "smtp.qq.com:465");
        settings.sinks.push(smtp);
        settings.prefix_consent = Some(NotificationDeliveryPrefixConsent {
            sink_id: "qq".to_owned(),
            destination: "me@qq.com".to_owned(),
            endpoint: "smtp.qq.com:465".to_owned(),
            accepted_at_ms: 3,
        });
        settings.canonicalize();
        assert!(settings.prefix_is_allowed_for("qq", "me@qq.com", "smtp.qq.com:465"));
        // A relay that moved to another host is another recipient as far as the
        // consent is concerned.
        settings.sinks[0].config = NotificationSinkConfig::Smtp {
            preset: None,
            host: "smtp.example.com".to_owned(),
            port: 587,
            tls: SmtpTls::StartTls,
            username: "me@qq.com".to_owned(),
        };
        settings.canonicalize();
        assert_eq!(settings.prefix_consent, None);
    }
}
