//! Load-time migration of the pre-#1460 delivery block (state-machine.md, "Local
//! Notification Delivery", Migration).
//!
//! The old block had one channel switch, one trigger set, one timing, and one prefix
//! consent naming a single sink. A sink written before #1460 has no `targets` field,
//! which is how it is recognized. Every rule here can only narrow what leaves the
//! device: a switched-off channel or sink gets no targets, only the sink the old
//! consent named keeps message text, and nothing becomes the entire-message mode.

use serde::Deserialize;

use super::{
    NotificationDeliveryContent, NotificationDeliveryContentConsent,
    NotificationDeliveryPartnerLabel, NotificationDeliverySettings, NotificationDeliveryTarget,
    NotificationDeliveryTargets, NotificationDeliveryTiming, NotificationSinkConfig,
    NotificationSinkSettings,
};

/// The prefix bound the pre-#1460 schema enforced. A migrated prefix never exceeds it,
/// even when the file holds a larger value, so migration cannot widen what was sent.
const LEGACY_MAX_PREFIX_CHARS: u32 = 200;

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NotificationDeliverySettingsWire {
    sinks: Vec<NotificationSinkWire>,
    // Pre-#1460 channel-wide fields, read once and never written back.
    enabled: Option<bool>,
    direct_messages: Option<bool>,
    mentions: Option<bool>,
    keywords: Option<bool>,
    only_when_unfocused: Option<bool>,
    dwell_secs: Option<u64>,
    min_interval_secs: Option<u64>,
    prefix_chars: Option<u32>,
    prefix_consent: Option<LegacyPrefixConsent>,
    watched: Option<Vec<NotificationDeliveryTarget>>,
}

#[derive(Deserialize)]
struct LegacyPrefixConsent {
    sink_id: String,
    destination: String,
    endpoint: String,
    accepted_at_ms: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NotificationSinkWire {
    id: String,
    destination: String,
    from_address: String,
    sender_name: String,
    config: NotificationSinkConfig,
    secret_binding: Option<String>,
    targets: Option<NotificationDeliveryTargets>,
    content: Option<NotificationDeliveryContent>,
    content_consent: Option<NotificationDeliveryContentConsent>,
    timing: Option<NotificationDeliveryTiming>,
    direct_partner_label: NotificationDeliveryPartnerLabel,
    // Pre-#1460 per-sink switch.
    enabled: Option<bool>,
}

impl NotificationDeliverySettingsWire {
    pub(super) fn into_settings(self) -> NotificationDeliverySettings {
        let legacy_timing = NotificationDeliveryTiming {
            dwell_secs: self
                .dwell_secs
                .unwrap_or(super::DEFAULT_NOTIFICATION_DELIVERY_DWELL_SECS),
            min_interval_secs: self
                .min_interval_secs
                .unwrap_or(super::DEFAULT_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS),
            only_when_unfocused: self.only_when_unfocused.unwrap_or(true),
        };
        let legacy_targets = NotificationDeliveryTargets {
            direct_messages: self.direct_messages.unwrap_or(true),
            mentions: self.mentions.unwrap_or(true),
            keywords: self.keywords.unwrap_or(true),
            watched: self.watched.clone().unwrap_or_default(),
        };
        let channel_enabled = self.enabled.unwrap_or(false);
        let legacy_prefix = self
            .prefix_chars
            .unwrap_or(super::DEFAULT_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES)
            .min(LEGACY_MAX_PREFIX_CHARS);
        let legacy_consent = self.prefix_consent;

        let sinks = self
            .sinks
            .into_iter()
            .map(|wire| {
                let is_legacy = wire.targets.is_none();
                let mut sink = NotificationSinkSettings {
                    id: wire.id,
                    destination: wire.destination,
                    from_address: wire.from_address,
                    sender_name: wire.sender_name,
                    config: wire.config,
                    secret_binding: wire.secret_binding,
                    targets: wire.targets.unwrap_or_default(),
                    content: wire.content.unwrap_or_default(),
                    content_consent: wire.content_consent,
                    timing: wire.timing.unwrap_or_default(),
                    direct_partner_label: wire.direct_partner_label,
                };
                if is_legacy {
                    migrate_legacy_sink(
                        &mut sink,
                        channel_enabled && wire.enabled.unwrap_or(false),
                        &legacy_targets,
                        &legacy_timing,
                        legacy_prefix,
                        legacy_consent.as_ref(),
                    );
                }
                sink
            })
            .collect();
        NotificationDeliverySettings { sinks }
    }
}

fn migrate_legacy_sink(
    sink: &mut NotificationSinkSettings,
    was_sending: bool,
    legacy_targets: &NotificationDeliveryTargets,
    legacy_timing: &NotificationDeliveryTiming,
    legacy_prefix: u32,
    legacy_consent: Option<&LegacyPrefixConsent>,
) {
    sink.targets = if was_sending {
        legacy_targets.clone()
    } else {
        NotificationDeliveryTargets::none()
    };
    sink.timing = legacy_timing.clone();
    sink.content = NotificationDeliveryContent::NamesOnly;
    sink.content_consent = None;

    // The id is compared after the same trim/lowercase the canonical form applies, so
    // an id the old file stored un-normalized still finds its consent.
    let consent_names_this_sink = legacy_consent.is_some_and(|consent| {
        consent.sink_id.trim().eq_ignore_ascii_case(sink.id.trim())
            && consent.destination.trim() == sink.destination.trim()
            && consent.endpoint.trim() == sink.endpoint().trim()
    });
    if let Some(consent) = legacy_consent.filter(|_| consent_names_this_sink && legacy_prefix > 0) {
        sink.content = NotificationDeliveryContent::Prefix {
            graphemes: legacy_prefix,
        };
        sink.content_consent = Some(NotificationDeliveryContentConsent {
            sink_kind: sink.config.kind().to_owned(),
            endpoint: consent.endpoint.trim().to_owned(),
            recipient: consent.destination.trim().to_owned(),
            accepted_at_ms: consent.accepted_at_ms,
        });
    }
}
