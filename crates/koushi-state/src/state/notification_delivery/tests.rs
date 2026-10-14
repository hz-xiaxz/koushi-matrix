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
    assert!(settings.watched.is_empty());
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
    assert!(!settings.prefix_is_allowed_for("home", "other@example.com", DEFAULT_RESEND_ENDPOINT));

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
fn the_watch_list_keeps_unique_targets_and_drops_what_is_not_an_id() {
    let mut settings = NotificationDeliverySettings {
        watched: vec![
            NotificationDeliveryTarget::Room {
                room_id: " !a:example.org ".to_owned(),
            },
            NotificationDeliveryTarget::Room {
                room_id: "!a:example.org".to_owned(),
            },
            NotificationDeliveryTarget::Space {
                space_id: "!a:example.org".to_owned(),
            },
            NotificationDeliveryTarget::Room {
                room_id: "a:example.org".to_owned(),
            },
            NotificationDeliveryTarget::Space {
                space_id: "#alias:example.org".to_owned(),
            },
            NotificationDeliveryTarget::Room {
                room_id: "!".to_owned(),
            },
        ],
        ..NotificationDeliverySettings::default()
    };
    settings.canonicalize();

    assert_eq!(settings.watched.len(), 3);
    assert_eq!(settings.watched[0].id(), "!a:example.org");
    assert!(settings.watches_room("!a:example.org"));
    // The same id as a space is a different target, so the space survives.
    assert!(settings.watches_space("!a:example.org"));
    assert!(settings.watches_space("#alias:example.org"));
    assert!(!settings.watches_room("#alias:example.org"));
    assert!(!settings.watches_room("a:example.org"));
    assert!(!settings.watches_room("!"));
}

#[test]
fn the_watch_list_is_capped() {
    let mut settings = NotificationDeliverySettings {
        watched: (0..MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS + 5)
            .map(|index| NotificationDeliveryTarget::Room {
                room_id: format!("!room{index}:example.org"),
            })
            .collect(),
        ..NotificationDeliverySettings::default()
    };
    settings.canonicalize();
    assert_eq!(
        settings.watched.len(),
        MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS
    );
    assert!(settings.watches_room("!room0:example.org"));
    assert!(!settings.watches_room("!room64:example.org"));
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
