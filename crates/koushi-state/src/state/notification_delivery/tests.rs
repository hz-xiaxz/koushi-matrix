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
        secret_binding: None,
    }
}

fn smtp_sink(host: &str, port: u16, username: &str) -> NotificationSinkSettings {
    NotificationSinkSettings {
        config: NotificationSinkConfig::Smtp {
            preset: None,
            host: host.to_owned(),
            port,
            tls: SmtpTls::Implicit,
            username: username.to_owned(),
        },
        ..sink("mail", "me@example.com")
    }
}

#[test]
fn a_credential_binding_outlives_the_recipient_and_the_sender_name_but_not_the_server() {
    let mut sink = sink("home", "me@example.com");
    sink.canonicalize();
    sink.secret_binding = Some(sink.credential_identity());
    assert!(sink.secret_binding.is_some());

    // Who receives the mail and what the sender is called are not part of the identity, so
    // editing them must never throw away a password the user has already entered.
    sink.destination = "other@example.com".to_owned();
    sink.from_address = "other@example.com".to_owned();
    sink.sender_name = "Other".to_owned();
    sink.enabled = false;
    sink.canonicalize();
    assert!(
        sink.secret_binding.is_some(),
        "the recipient and the sender name are not the credential's identity"
    );

    // The endpoint is the server the credential is offered to, so moving must retire it.
    sink.config = NotificationSinkConfig::Resend {
        endpoint: "https://relay.example.com/send".to_owned(),
    };
    sink.canonicalize();
    assert_eq!(
        sink.secret_binding, None,
        "another endpoint is another server and must not inherit the password"
    );
}

#[test]
fn an_smtp_credential_is_retired_when_the_server_or_the_account_changes() {
    let mut sink = smtp_sink("smtp.gmail.com", 465, "me@example.com");
    sink.canonicalize();
    sink.secret_binding = Some(sink.credential_identity());

    // Re-canonicalizing the same configuration keeps the credential.
    sink.canonicalize();
    assert!(sink.secret_binding.is_some());

    // Renaming the sink keeps it too: the identity is the server and the account, not the
    // name of the button that filled the fields in.
    let mut renamed = sink.clone();
    renamed.id = "work".to_owned();
    renamed.canonicalize();
    assert!(renamed.secret_binding.is_some());

    for (host, port, username, why) in [
        ("smtp.gmail.com", 587, "me@example.com", "another port"),
        ("smtp.qq.com", 465, "me@example.com", "another server"),
        (
            "smtp.gmail.com",
            465,
            "other@example.com",
            "another account",
        ),
    ] {
        let mut changed = sink.clone();
        changed.config = smtp_sink(host, port, username).config;
        changed.canonicalize();
        assert_eq!(
            changed.secret_binding, None,
            "{why} must not receive the password entered for the previous one"
        );
    }
}

#[test]
fn a_sink_saved_before_the_credential_binding_existed_still_loads() {
    // The binding is newer than the rest of the sink, so a file written by an earlier
    // build must still load. A missing binding is not a wildcard: it means the stored
    // credential, if there is one, is not usable for this sink.
    let delivery: NotificationDeliverySettings = serde_json::from_str(
        r#"{"enabled":true,"sinks":[{"id":"home","enabled":true,"destination":"me@example.com","from_address":"koushi@example.com","sender_name":"Koushi","config":{"kind":"resend","endpoint":"https://relay.example.com/emails"}}]}"#,
    )
    .expect("a settings file written before the binding existed must still load");
    assert_eq!(delivery.sinks.len(), 1);
    assert_eq!(delivery.sinks[0].secret_binding, None);
    assert_eq!(delivery.sinks[0].destination, "me@example.com");
}

#[test]
fn a_binding_that_does_not_name_this_configuration_is_dropped_on_load() {
    // The settings file can be edited while the app is closed, so a binding that does not
    // match is dropped at load rather than trusted.
    let mut smtp = smtp_sink("smtp.gmail.com", 465, "me@example.com");
    smtp.secret_binding = Some("smtp:elsewhere.example.com:465:someone@example.com".to_owned());
    smtp.canonicalize();
    assert_eq!(smtp.secret_binding, None);

    let mut resend = sink("home", "me@example.com");
    resend.secret_binding = Some("resend:https://elsewhere.example.com/send".to_owned());
    resend.canonicalize();
    assert_eq!(resend.secret_binding, None);
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

/// #1427: what a candidate holds is bounded before the dwell can carry it, and the
/// bound never splits a character.
#[test]
fn prefix_source_is_bounded_on_a_character_boundary() {
    assert_eq!(bounded_notification_delivery_prefix_source(""), None);
    assert_eq!(
        bounded_notification_delivery_prefix_source("short").as_deref(),
        Some("short")
    );

    let at_cap = "a".repeat(MAX_NOTIFICATION_DELIVERY_PREFIX_SOURCE_CHARS);
    assert_eq!(
        bounded_notification_delivery_prefix_source(&at_cap).as_deref(),
        Some(at_cap.as_str())
    );

    let beyond = format!(
        "{}tail",
        "日".repeat(MAX_NOTIFICATION_DELIVERY_PREFIX_SOURCE_CHARS)
    );
    let bounded = bounded_notification_delivery_prefix_source(&beyond).expect("a long body");
    assert_eq!(
        bounded.chars().count(),
        MAX_NOTIFICATION_DELIVERY_PREFIX_SOURCE_CHARS
    );
    assert!(bounded.chars().all(|character| character == '日'));

    // Whitespace is kept here: dropping a prefix that renders to nothing is the
    // digest's decision, not this bound's.
    assert_eq!(
        bounded_notification_delivery_prefix_source("   ").as_deref(),
        Some("   ")
    );
}
