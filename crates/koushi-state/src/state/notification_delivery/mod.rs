//! Device-local notification delivery settings (#1427, #1460).
//!
//! "Sent from this device" is a list of destinations. Each destination is one sink -
//! Resend, an SMTP relay such as QQ Mail, Gmail, or iCloud, or WeChat through
//! ServerChan Turbo - with its own targets, content mode, timing, and outcome. There is
//! no channel-wide and no per-destination enable switch: a destination is eligible when
//! its setup is complete, its credential is stored, and one of its targets is set. The
//! route is app-scoped: it never asks the Matrix account for a destination, so it works
//! on a homeserver that supports neither email 3PIDs nor pushers. Every value here is
//! non-secret. A sink's credential lives in the credential store, keyed by the sink id,
//! and is reported to the product only as a configured flag (state-machine.md, "Local
//! Notification Delivery").
//!
//! [`NotificationDeliveryState`] is the device-local *runtime* projection of that
//! route: the per-sink configured flag and last outcome, plus the mailbox preset
//! catalog the settings screen offers. It is app scope and deliberately not part of
//! the settings file - a restart re-observes the credential store and re-arms every
//! timer - and it never carries a secret value.

use serde::{Deserialize, Serialize};

mod migration;

/// How long a room must stay in a qualifying attention state before a destination
/// delivers a notification about it. Five minutes by default: long enough that a
/// conversation the user is already reading is never sent.
pub const DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS: u64 = 300;
/// The shortest gap between two deliveries of the same destination. Fifteen minutes by
/// default, so a busy group cannot fill an inbox.
pub const DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS: u64 = 900;
/// A day is the longest dwell that still means "while you were away"; anything
/// longer would deliver a state the user has long since seen.
pub const MAX_NOTIFICATION_DELIVERY_DWELL_SECS: u64 = 86_400;
pub const MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS: u64 = 86_400;
/// The start-of-message mode is capped so a consent given for a glance cannot carry a
/// page of text; the entire-message mode is the explicit way to send more.
pub const MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES: u32 = 500;
pub const DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES: u32 = 80;
/// How much of a room's newest message text a delivery candidate holds (#1460).
///
/// This is the source bound, not what is sent: the provider budget below cuts the
/// rendered text at a grapheme boundary. A source cut here is flagged, so the rendered
/// text says it was truncated and is never presented as the entire message.
pub const MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS: usize = 4000;
/// Per-room text budget of a mail destination, in grapheme clusters.
pub const NOTIFICATION_DELIVERY_MAIL_TEXT_BUDGET: usize = 4000;
/// Per-room text budget of a ServerChan destination, in grapheme clusters. The relay
/// shows the text on a web page behind the WeChat card and documents a 32 KB body;
/// a smaller budget keeps a five-room digest well inside it.
pub const NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET: usize = 1000;
/// Destinations are a short list the user configures by hand; the cap keeps a corrupted
/// or hostile settings file from turning one settings load into unbounded work.
pub const MAX_NOTIFICATION_DELIVERY_SINKS: usize = 8;
/// The watch list is capped as well: a candidate set is built on every attention
/// update, so the membership lookup has to stay bounded.
pub const MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS: usize = 64;
pub const MAX_NOTIFICATION_DELIVERY_FIELD_CHARS: usize = 320;
/// The longest credential revision a binding may carry, in hex digits.
pub const MAX_NOTIFICATION_CREDENTIAL_REVISION_CHARS: usize = 32;

/// A room's newest message text, bounded for a delivery candidate (#1460).
#[derive(Clone, Eq, PartialEq)]
pub struct BoundedNotificationDeliveryText {
    pub text: String,
    /// Whether the source was longer than
    /// [`MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS`].
    pub truncated_at_source: bool,
}

impl std::fmt::Debug for BoundedNotificationDeliveryText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BoundedNotificationDeliveryText")
            .field("text", &"<redacted>")
            .field("truncated_at_source", &self.truncated_at_source)
            .finish()
    }
}

/// Bound message text before a delivery candidate holds it.
///
/// The cut keeps at most [`MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS`] characters
/// and falls on a grapheme boundary, so a base letter never loses the combining mark past
/// the bound and an emoji sequence is never split. An empty body is `None` because there
/// is nothing to render; a body of only whitespace is kept, and `digest` is the one that
/// drops it.
pub fn bounded_notification_delivery_message_source(
    text: &str,
) -> Option<BoundedNotificationDeliveryText> {
    if text.is_empty() {
        return None;
    }
    use unicode_segmentation::UnicodeSegmentation;
    let mut kept_chars = 0;
    for (start, grapheme) in text.grapheme_indices(true) {
        kept_chars += grapheme.chars().count();
        if kept_chars > MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS {
            return Some(BoundedNotificationDeliveryText {
                text: text[..start].to_owned(),
                truncated_at_source: true,
            });
        }
    }
    Some(BoundedNotificationDeliveryText {
        text: text.to_owned(),
        truncated_at_source: false,
    })
}

const MAX_NOTIFICATION_SINK_ID_BYTES: usize = 64;

/// Default endpoint of the Resend sink, matching `koushi_notify::RESEND_ENDPOINT`.
/// It is repeated here because `koushi-state` is a data crate with no dependency on
/// the senders; the core compares the two when it builds a delivery.
pub const DEFAULT_RESEND_ENDPOINT: &str = "https://api.resend.com/emails";

/// The sender Resend lets every account use without a domain of its own (#1468). Resend
/// delivers mail from it only to the address registered on the Resend account, which is
/// exactly the "notify myself" case, so an empty Resend sender falls back to it.
pub const DEFAULT_RESEND_FROM_ADDRESS: &str = "onboarding@resend.dev";

/// The sender name a mail destination uses when the user leaves it empty (#1468).
pub const DEFAULT_NOTIFICATION_SENDER_NAME: &str = "Koushi";
/// Default endpoint of the ServerChan Turbo sink, matching
/// `koushi_notify::SERVERCHAN_ENDPOINT`. The request path appends the SendKey, so the
/// endpoint itself never names the recipient.
pub const DEFAULT_SERVERCHAN_ENDPOINT: &str = "https://sctapi.ftqq.com";

/// The destinations this device may deliver through.
///
/// There is no enable switch: an empty list sends nothing, and a destination with no
/// target or no credential sends nothing. Loading a pre-#1460 file runs the
/// conservative migration in [`migration`].
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct NotificationDeliverySettings {
    pub sinks: Vec<NotificationSinkSettings>,
}

impl<'de> Deserialize<'de> for NotificationDeliverySettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        migration::NotificationDeliverySettingsWire::deserialize(deserializer)
            .map(migration::NotificationDeliverySettingsWire::into_settings)
    }
}

impl NotificationDeliverySettings {
    pub fn sink(&self, id: &str) -> Option<&NotificationSinkSettings> {
        self.sinks.iter().find(|sink| sink.id == id)
    }

    pub fn sink_mut(&mut self, id: &str) -> Option<&mut NotificationSinkSettings> {
        self.sinks.iter_mut().find(|sink| sink.id == id)
    }

    /// Replace this block with a patched one.
    ///
    /// Only the core may name a credential: it writes a binding after it stored that
    /// credential, so a patch from the renderer keeps, for each sink id, the binding the
    /// core last wrote. A renderer echoing a binding it saw before a SendKey was replaced
    /// would otherwise restore the old revision and the consent given for its owner.
    pub fn apply_patch(
        &mut self,
        incoming: &NotificationDeliverySettings,
        bindings_from_core: bool,
    ) {
        let mut patched = incoming.clone();
        patched.canonicalize();
        if !bindings_from_core {
            for sink in &mut patched.sinks {
                sink.secret_binding = self
                    .sink(&sink.id)
                    .and_then(|current| current.secret_binding.clone());
            }
            // The kept binding may not name this configuration any more, and a consent the
            // renderer sent may name a revision the kept binding does not: both fail closed.
            patched.canonicalize();
        }
        *self = patched;
    }

    /// Apply what this device must never depend on a well-formed settings file for:
    /// bounded numbers, bounded strings, unique sink ids and watch targets, and a
    /// content consent that still matches its destination. Called on every load and
    /// every patch.
    pub fn canonicalize(&mut self) {
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
    }
}

/// Deserialize hook for the settings file. A delivery block read from disk is migrated,
/// bounded, deduplicated, and de-consented before any product code can see it, so a
/// hand-edited or older file cannot widen what leaves the device.
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
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
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

/// What one destination is sent for: one list of categories and watched targets.
///
/// Keywords are Matrix `kind: content` push rules on the homeserver, shared with every
/// client of the account; a hit ranks as mention-kind in the attention projection. A
/// watched target overrides the categories but never the read, mute, or focus
/// suppression, and a watched space covers rooms added to it later.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationDeliveryTargets {
    pub direct_messages: bool,
    /// Mentions and replies that mention the user.
    pub mentions: bool,
    pub keywords: bool,
    pub watched: Vec<NotificationDeliveryTarget>,
}

impl Default for NotificationDeliveryTargets {
    fn default() -> Self {
        Self {
            direct_messages: true,
            mentions: true,
            keywords: true,
            watched: Vec::new(),
        }
    }
}

impl NotificationDeliveryTargets {
    /// Nothing selected: what a migrated destination gets when it was switched off.
    pub fn none() -> Self {
        Self {
            direct_messages: false,
            mentions: false,
            keywords: false,
            watched: Vec::new(),
        }
    }

    pub fn has_any(&self) -> bool {
        self.direct_messages || self.mentions || self.keywords || !self.watched.is_empty()
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

    fn canonicalize(&mut self) {
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
    }
}

/// How much message text a destination may carry (#1460).
///
/// Anything but [`NamesOnly`](Self::NamesOnly) needs a matching
/// [`NotificationDeliveryContentConsent`]; without one the destination is names only.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationDeliveryContent {
    /// Room name, kind, parent space name, and counts. No message text.
    #[default]
    NamesOnly,
    /// The first `graphemes` grapheme clusters of the latest eligible text message.
    Prefix { graphemes: u32 },
    /// The latest eligible text message, up to the provider budget. Text cut by a
    /// budget is marked truncated and is never called the entire message.
    Full,
}

impl NotificationDeliveryContent {
    pub fn carries_text(self) -> bool {
        !matches!(self, Self::NamesOnly)
    }

    fn canonicalize(&mut self) {
        if let Self::Prefix { graphemes } = self {
            *graphemes = (*graphemes).min(MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES);
            if *graphemes == 0 {
                *self = Self::NamesOnly;
            }
        }
    }
}

/// How a digest names the other member of a direct message (#1492).
///
/// A display name is never used: it is not unique and often equals the bare localpart, so
/// the recipient could not tell who wrote.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationDeliveryPartnerLabel {
    /// The partner's personal local alias when one is set, else their Matrix user id. The
    /// alias is this device's private label, so choosing this sends it to the provider.
    #[default]
    LocalAlias,
    /// Always the partner's full Matrix user id.
    MatrixId,
}

/// The user's acceptance that one destination receives message text in plaintext.
///
/// Bound to the sink kind, its endpoint, and its recipient - the recipient email address,
/// or for ServerChan the SendKey owner. Saving or clearing a ServerChan SendKey is a
/// recipient change, which the core applies by dropping this consent. A change to any
/// of the three drops it and returns the destination to names only.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliveryContentConsent {
    pub sink_kind: String,
    pub endpoint: String,
    pub recipient: String,
    /// When the user accepted, in milliseconds since the Unix epoch.
    pub accepted_at_ms: i64,
}

/// When one destination sends.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationDeliveryTiming {
    pub dwell_secs: u64,
    pub min_interval_secs: u64,
    /// Hold delivery while this app is focused, the way the OS suppresses a banner the
    /// user is already looking at. Recommended, and the default.
    pub only_when_unfocused: bool,
}

impl Default for NotificationDeliveryTiming {
    fn default() -> Self {
        Self {
            dwell_secs: DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS,
            min_interval_secs: DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS,
            only_when_unfocused: true,
        }
    }
}

impl NotificationDeliveryTiming {
    fn canonicalize(&mut self) {
        self.dwell_secs = self.dwell_secs.min(MAX_NOTIFICATION_DELIVERY_DWELL_SECS);
        self.min_interval_secs = self
            .min_interval_secs
            .min(MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS);
    }
}

/// One destination: one configured way out of the device.
///
/// The id names the credential in the credential store and is the only identity a
/// sink has; two sinks never share one.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSinkSettings {
    pub id: String,
    /// Where the message goes: the mailbox for a mail sink. Empty for ServerChan, whose
    /// recipient is whoever owns the SendKey.
    pub destination: String,
    pub from_address: String,
    /// The name the recipient sees. Empty falls back to the bare address.
    pub sender_name: String,
    pub config: NotificationSinkConfig,
    /// The server and account the stored credential was entered for, in the shape
    /// [`credential_binding`](Self::credential_binding) renders: the
    /// [`credential_identity`](Self::credential_identity) plus an opaque revision minted
    /// each time a credential is saved. The credential store is keyed by the sink id
    /// alone, so without the identity a preset, host, port or username change would keep
    /// the previous mailbox's password and offer it to the new server's `AUTH LOGIN`;
    /// without the revision a replaced ServerChan SendKey, whose owner is the recipient,
    /// would look like the same destination. Rust owns this field: the user never types
    /// it.
    pub secret_binding: Option<String>,
    pub targets: NotificationDeliveryTargets,
    pub content: NotificationDeliveryContent,
    pub content_consent: Option<NotificationDeliveryContentConsent>,
    pub timing: NotificationDeliveryTiming,
    /// How a direct message's other member is named (#1492).
    pub direct_partner_label: NotificationDeliveryPartnerLabel,
}

impl NotificationSinkSettings {
    /// The non-secret string a content consent binds to, so that editing the endpoint
    /// asks for the consent again.
    pub fn endpoint(&self) -> String {
        match &self.config {
            NotificationSinkConfig::Resend { endpoint }
            | NotificationSinkConfig::ServerChan { endpoint } => endpoint.clone(),
            NotificationSinkConfig::Smtp { host, port, .. } => format!("{host}:{port}"),
        }
    }

    /// The server and account a credential for this sink authenticates. Two sinks whose
    /// identity differs must never be able to exchange a stored credential, which is why
    /// the binding is compared against this string rather than against the sink id.
    pub fn credential_identity(&self) -> String {
        match &self.config {
            NotificationSinkConfig::Resend { .. } => format!("resend:{}", self.endpoint()),
            NotificationSinkConfig::ServerChan { .. } => {
                format!("serverchan:{}", self.endpoint())
            }
            NotificationSinkConfig::Smtp {
                host,
                port,
                username,
                ..
            } => format!("smtp:{host}:{port}:{username}"),
        }
    }

    /// The binding a credential saved now under `revision` carries.
    pub fn credential_binding(&self, revision: &str) -> String {
        format!("{}#{revision}", self.credential_identity())
    }

    /// The revision of the bound credential, when the binding names the current
    /// configuration. A binding written before revisions existed is exactly the identity
    /// and has the empty revision.
    fn credential_revision(&self) -> Option<&str> {
        let binding = self.secret_binding.as_deref()?;
        let rest = binding.strip_prefix(self.credential_identity().as_str())?;
        if rest.is_empty() {
            return Some("");
        }
        let revision = rest.strip_prefix('#')?;
        let well_formed = !revision.is_empty()
            && revision.len() <= MAX_NOTIFICATION_CREDENTIAL_REVISION_CHARS
            && revision.bytes().all(|byte| byte.is_ascii_hexdigit());
        well_formed.then_some(revision)
    }

    /// The recipient a content consent binds to. For ServerChan it is the SendKey's
    /// owner, which the settings never see; the bound credential's revision stands in
    /// for it, so saving another SendKey (a new revision) or clearing it (none) names a
    /// different recipient and the consent no longer matches.
    pub fn consent_recipient(&self) -> String {
        match &self.config {
            NotificationSinkConfig::ServerChan { .. } => match self.credential_revision() {
                Some(revision) if !revision.is_empty() => format!("sendkey#{revision}"),
                _ => String::new(),
            },
            _ => self.destination.clone(),
        }
    }

    /// The consent that would cover this destination exactly as it is configured now.
    pub fn consent_for_current(&self, accepted_at_ms: i64) -> NotificationDeliveryContentConsent {
        NotificationDeliveryContentConsent {
            sink_kind: self.config.kind().to_owned(),
            endpoint: self.endpoint(),
            recipient: self.consent_recipient(),
            accepted_at_ms,
        }
    }

    fn consent_matches(&self) -> bool {
        self.content_consent.as_ref().is_some_and(|consent| {
            consent.sink_kind == self.config.kind()
                && consent.endpoint == self.endpoint()
                && consent.recipient == self.consent_recipient()
        })
    }

    /// The content mode this destination may actually use: its chosen mode while the
    /// consent matches, names only otherwise.
    pub fn effective_content(&self) -> NotificationDeliveryContent {
        if self.content.carries_text() && self.consent_matches() {
            self.content
        } else {
            NotificationDeliveryContent::NamesOnly
        }
    }

    /// Drop the content consent and return to names only: the recipient may have
    /// changed in a way the settings cannot see (a new ServerChan SendKey).
    pub fn revoke_content_consent(&mut self) {
        self.content_consent = None;
        self.content = NotificationDeliveryContent::NamesOnly;
    }

    /// The per-room text budget of this destination's provider, in grapheme clusters.
    pub fn text_budget_graphemes(&self) -> usize {
        match self.config {
            NotificationSinkConfig::ServerChan { .. } => {
                NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET
            }
            _ => NOTIFICATION_DELIVERY_MAIL_TEXT_BUDGET,
        }
    }

    /// Whether this destination has a credential bound to its current configuration.
    /// The core still checks the store before it reports the destination configured.
    pub fn has_bound_secret(&self) -> bool {
        self.secret_binding.is_some()
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
                if self.from_address.is_empty() {
                    self.from_address = DEFAULT_RESEND_FROM_ADDRESS.to_owned();
                }
            }
            NotificationSinkConfig::ServerChan { endpoint } => {
                *endpoint = trimmed_field(endpoint).trim_end_matches('/').to_owned();
                if endpoint.is_empty() {
                    *endpoint = DEFAULT_SERVERCHAN_ENDPOINT.to_owned();
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
        if self.sender_name.is_empty() {
            self.sender_name = DEFAULT_NOTIFICATION_SENDER_NAME.to_owned();
        }
        if matches!(self.config, NotificationSinkConfig::ServerChan { .. }) {
            // The recipient is the SendKey owner; an address field would be a stale copy
            // of a mail destination this sink no longer has.
            self.destination.clear();
            self.from_address.clear();
            self.sender_name.clear();
        }
        // A credential is only usable for the server and account it was entered for, so a
        // binding that no longer matches the configuration is dropped. This runs after the
        // fields above have been normalized and on every load and every patch, so a
        // settings file edited while the app was closed fails closed here too.
        if self.credential_revision().is_none() {
            self.secret_binding = None;
        }
        self.targets.canonicalize();
        self.timing.canonicalize();
        self.content.canonicalize();
        // A ServerChan consent names the SendKey owner; without a revisioned key there is
        // no owner it could still name.
        let serverchan_without_key =
            matches!(self.config, NotificationSinkConfig::ServerChan { .. })
                && self.consent_recipient().is_empty();
        if !self.consent_matches() || serverchan_without_key {
            self.revoke_content_consent();
        }
    }

    /// Whether the sink carries enough to be attempted at all. A sink that fails this
    /// is dropped rather than shown as configured-but-broken.
    fn is_usable(&self) -> bool {
        is_valid_notification_sink_id(&self.id)
            && match &self.config {
                NotificationSinkConfig::Resend { endpoint } => {
                    !endpoint.is_empty()
                        && !self.destination.is_empty()
                        && !self.from_address.is_empty()
                }
                NotificationSinkConfig::Smtp { host, port, .. } => {
                    !host.is_empty()
                        && *port > 0
                        && !self.destination.is_empty()
                        && !self.from_address.is_empty()
                }
                NotificationSinkConfig::ServerChan { endpoint } => !endpoint.is_empty(),
            }
    }
}

/// The transport a sink speaks. Tagged by `kind` so a settings file and the frontend
/// DTO carry the same shape the core dispatches on.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
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
    /// WeChat through ServerChan Turbo. The credential is the `SCT…` SendKey, which
    /// names the recipient; the endpoint is overridable only for a local stub.
    ServerChan { endpoint: String },
}

impl NotificationSinkConfig {
    /// The stable kind name a consent binds to.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Resend { .. } => "resend",
            Self::Smtp { .. } => "smtp",
            Self::ServerChan { .. } => "serverChan",
        }
    }
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

/// The device-local delivery channel as the frontend sees it (#1427).
///
/// App scope, and deliberately outside [`NotificationDeliverySettings`]: the settings
/// file holds what the user chose, this holds what this run of the app has observed -
/// whether each sink's credential is in the store, and how its last send ended. It is
/// never persisted, so restarting re-reads the credential store instead of trusting a
/// stale flag, and it holds no secret: a sink secret is only ever read at send time and
/// only travels into the sink.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliveryState {
    /// One entry per sink in [`NotificationDeliverySettings::sinks`], in that order.
    pub sinks: Vec<NotificationDeliverySinkState>,
    /// The mailbox presets this build offers; see
    /// [`NotificationDeliveryPresetState`].
    pub presets: Vec<NotificationDeliveryPresetState>,
}

/// One delivery sink's runtime facts, keyed by the same sink id the settings use.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliverySinkState {
    pub sink_id: String,
    /// Whether the credential store holds a secret for this sink. A value never
    /// reaches this flag, only its presence.
    pub secret_configured: bool,
    /// How this sink's most recent send ended, if it has sent in this run. The
    /// frontend must not read a sink as working while this is a failure.
    pub last_outcome: Option<NotificationDeliveryOutcomeState>,
    /// Whether this device deleted a stored credential for this sink because it was
    /// entered for a server or account the sink no longer names. It is a fact about
    /// the credential and never the credential, and it is the only way the frontend
    /// can learn about a deletion that happened while the settings were being loaded
    /// rather than during an edit. A sink that never had a credential does not carry
    /// the flag, and saving a new one clears it.
    #[serde(default)]
    pub secret_retired: bool,
}

/// One sink's most recent send, as a typed fact with the time it was observed.
///
/// The variants mirror the senders' own verdicts
/// (`koushi_notify::NotificationDeliveryFailure`) one for one, so the frontend renders
/// one catalog sentence per case instead of guessing from a string. `at_ms` is when
/// this device observed the outcome, in milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationDeliveryOutcomeState {
    /// The provider accepted the request; never a claim that it was received.
    Accepted {
        at_ms: u64,
    },
    InvalidSecret {
        at_ms: u64,
    },
    NotPermitted {
        at_ms: u64,
    },
    Rejected {
        at_ms: u64,
    },
    RateLimited {
        at_ms: u64,
        /// The provider's own delay, when it gave one. Absent means this device
        /// chooses its own backoff.
        retry_after_secs: Option<u64>,
    },
    Transport {
        at_ms: u64,
    },
}

/// One mailbox preset a user can pick instead of filling in a relay by hand.
///
/// A copy of the sender crate's own catalog, projected so the settings screen can offer
/// it without depending on a sender: the host, port, and TLS mode are the provider's
/// published coordinates, not a secret.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationDeliveryPresetState {
    /// Stable id the settings and the credential store key on.
    pub id: String,
    /// The vendor's own proper name, which is not translated: a product name reads the
    /// same in every locale.
    pub label: String,
    pub host: String,
    pub port: u16,
    pub tls: SmtpTls,
    /// Whether the relay authenticates the full address rather than its local part.
    pub username_is_full_address: bool,
    /// Catalog key for the sentence that tells the user which of the provider's own
    /// credentials to go and fetch. Passed through unchanged; the frontend resolves it.
    pub secret_hint: String,
}

#[cfg(test)]
mod tests;

// Hand-written `Debug` (#1460): addresses, SMTP usernames, endpoints, consent
// recipients, and watched Matrix ids are private routing data, and these types sit
// inside the settings and state trees that get formatted whole. Only kinds, counts,
// booleans, and the sink id (an opaque local name) are shown.

impl std::fmt::Debug for NotificationDeliveryTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Room { .. } => formatter.write_str("Room(..)"),
            Self::Space { .. } => formatter.write_str("Space(..)"),
        }
    }
}

impl std::fmt::Debug for NotificationDeliveryTargets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationDeliveryTargets")
            .field("direct_messages", &self.direct_messages)
            .field("mentions", &self.mentions)
            .field("keywords", &self.keywords)
            .field("watched", &self.watched.len())
            .finish()
    }
}

impl std::fmt::Debug for NotificationDeliveryContentConsent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationDeliveryContentConsent")
            .field("sink_kind", &self.sink_kind)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for NotificationSinkConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}(..)", self.kind())
    }
}

impl std::fmt::Debug for NotificationSinkSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationSinkSettings")
            .field("id", &self.id)
            .field("config", &self.config)
            .field("has_destination", &!self.destination.is_empty())
            .field("secret_bound", &self.secret_binding.is_some())
            .field("targets", &self.targets)
            .field("content", &self.content)
            .field("content_consent", &self.content_consent.is_some())
            .field("timing", &self.timing)
            .finish()
    }
}
