use super::*;

fn sink(id: &str, destination: &str) -> NotificationSinkSettings {
    NotificationSinkSettings {
        id: id.to_owned(),
        destination: destination.to_owned(),
        from_address: "koushi@example.com".to_owned(),
        sender_name: "Koushi".to_owned(),
        config: NotificationSinkConfig::Resend {
            endpoint: DEFAULT_RESEND_ENDPOINT.to_owned(),
        },
        secret_binding: None,
        ..NotificationSinkSettings::default()
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
fn a_new_destination_sends_names_only_and_nothing_is_configured_by_default() {
    let settings = NotificationDeliverySettings::default();
    assert!(settings.sinks.is_empty());

    let sink = sink("home", "me@example.com");
    assert_eq!(sink.content, NotificationDeliveryContent::NamesOnly);
    assert_eq!(sink.content_consent, None);
    assert!(sink.targets.has_any());
    assert_eq!(sink.timing.dwell_secs, 300);
    assert_eq!(sink.timing.min_interval_secs, 900);
    assert!(sink.timing.only_when_unfocused);

    // A chosen mode without a consent must never let message text out.
    let mut chosen = sink.clone();
    chosen.content = NotificationDeliveryContent::Full;
    assert_eq!(
        chosen.effective_content(),
        NotificationDeliveryContent::NamesOnly
    );
}

#[test]
fn canonicalize_bounds_numbers_fields_and_sinks() {
    let mut first = sink("Home", " me@example.com ");
    first.timing.dwell_secs = u64::MAX;
    first.timing.min_interval_secs = u64::MAX;
    first.content = NotificationDeliveryContent::Prefix {
        graphemes: u32::MAX,
    };
    // The consent names the trimmed recipient, which is what the stored form holds.
    let mut consent = first.consent_for_current(1);
    consent.recipient = "me@example.com".to_owned();
    first.content_consent = Some(consent);
    let mut settings = NotificationDeliverySettings {
        sinks: vec![
            first,
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
    };
    settings.canonicalize();

    let home = &settings.sinks[0];
    assert_eq!(home.timing.dwell_secs, MAX_NOTIFICATION_DELIVERY_DWELL_SECS);
    assert_eq!(
        home.timing.min_interval_secs,
        MAX_NOTIFICATION_DELIVERY_MIN_INTERVAL_SECS
    );
    assert_eq!(
        home.content,
        NotificationDeliveryContent::Prefix {
            graphemes: MAX_NOTIFICATION_DELIVERY_PREFIX_GRAPHEMES
        }
    );
    assert_eq!(settings.sinks.len(), MAX_NOTIFICATION_DELIVERY_SINKS);
    // The first mention of an id wins, and its fields are trimmed.
    assert_eq!(home.id, "home");
    assert_eq!(home.destination, "me@example.com");
    assert!(!settings.sinks.iter().any(|sink| sink.id == "bad id"));
    assert!(
        !settings
            .sinks
            .iter()
            .any(|sink| sink.id == "no-destination")
    );
    assert!(settings.sinks.iter().any(|sink| sink.id == "s12"));
    assert!(!settings.sinks.iter().any(|sink| sink.id == "s13"));
}

#[test]
fn a_consent_dies_with_the_kind_endpoint_or_recipient_it_was_given_for() {
    let mut consented = sink("home", "me@example.com");
    consented.content = NotificationDeliveryContent::Full;
    consented.content_consent = Some(consented.consent_for_current(1));
    consented.canonicalize();
    assert_eq!(
        consented.effective_content(),
        NotificationDeliveryContent::Full
    );

    let mut recipient = consented.clone();
    recipient.destination = "other@example.com".to_owned();
    recipient.canonicalize();
    assert_eq!(recipient.content_consent, None);
    assert_eq!(recipient.content, NotificationDeliveryContent::NamesOnly);

    let mut endpoint = consented.clone();
    endpoint.config = NotificationSinkConfig::Resend {
        endpoint: "https://relay.example.com/send".to_owned(),
    };
    endpoint.canonicalize();
    assert_eq!(endpoint.content_consent, None);

    let mut kind = consented.clone();
    kind.config = smtp_sink("smtp.example.com", 465, "me").config;
    kind.canonicalize();
    assert_eq!(kind.content_consent, None);
    assert_eq!(kind.content, NotificationDeliveryContent::NamesOnly);
}

#[test]
fn a_serverchan_destination_has_no_address_and_its_consent_needs_a_bound_key() {
    let mut wechat = NotificationSinkSettings {
        id: "wechat".to_owned(),
        destination: "stale@example.com".to_owned(),
        config: NotificationSinkConfig::ServerChan {
            endpoint: " https://sctapi.ftqq.com/ ".to_owned(),
        },
        ..NotificationSinkSettings::default()
    };
    wechat.canonicalize();
    assert!(wechat.is_usable());
    assert_eq!(wechat.destination, "");
    assert_eq!(wechat.endpoint(), DEFAULT_SERVERCHAN_ENDPOINT);
    assert_eq!(
        wechat.credential_identity(),
        "serverchan:https://sctapi.ftqq.com"
    );
    assert_eq!(
        wechat.text_budget_graphemes(),
        NOTIFICATION_DELIVERY_SERVERCHAN_TEXT_BUDGET
    );

    // Without a bound SendKey there is no recipient a consent could name.
    wechat.content = NotificationDeliveryContent::Full;
    wechat.content_consent = Some(wechat.consent_for_current(1));
    wechat.canonicalize();
    assert_eq!(wechat.content_consent, None);

    wechat.secret_binding = Some(wechat.credential_identity());
    wechat.content = NotificationDeliveryContent::Full;
    wechat.content_consent = Some(wechat.consent_for_current(2));
    wechat.canonicalize();
    assert_eq!(
        wechat.effective_content(),
        NotificationDeliveryContent::Full
    );
    // A new SendKey is a new recipient; the core revokes on save.
    wechat.revoke_content_consent();
    assert_eq!(
        wechat.effective_content(),
        NotificationDeliveryContent::NamesOnly
    );
}

#[test]
fn the_watch_list_keeps_unique_targets_and_drops_what_is_not_an_id() {
    let mut targets = NotificationDeliveryTargets {
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
        ..NotificationDeliveryTargets::none()
    };
    targets.canonicalize();

    assert_eq!(targets.watched.len(), 3);
    assert_eq!(targets.watched[0].id(), "!a:example.org");
    assert!(targets.watches_room("!a:example.org"));
    assert!(targets.watches_space("!a:example.org"));
    assert!(targets.watches_space("#alias:example.org"));
    assert!(!targets.watches_room("#alias:example.org"));
    assert!(!targets.watches_room("a:example.org"));
    assert!(targets.has_any(), "a watched target alone is a target");
}

#[test]
fn the_watch_list_is_capped() {
    let mut targets = NotificationDeliveryTargets {
        watched: (0..MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS + 5)
            .map(|index| NotificationDeliveryTarget::Room {
                room_id: format!("!room{index}:example.org"),
            })
            .collect(),
        ..NotificationDeliveryTargets::none()
    };
    targets.canonicalize();
    assert_eq!(
        targets.watched.len(),
        MAX_NOTIFICATION_DELIVERY_WATCHED_TARGETS
    );
    assert!(targets.watches_room("!room0:example.org"));
    assert!(!targets.watches_room("!room64:example.org"));
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
    let mut smtp = sink("qq", "me@qq.com");
    smtp.config = NotificationSinkConfig::Smtp {
        preset: Some("qq-mail".to_owned()),
        host: "smtp.qq.com".to_owned(),
        port: 465,
        tls: SmtpTls::Implicit,
        username: "me@qq.com".to_owned(),
    };
    assert_eq!(smtp.endpoint(), "smtp.qq.com:465");
    smtp.content = NotificationDeliveryContent::Prefix { graphemes: 80 };
    smtp.content_consent = Some(smtp.consent_for_current(3));
    smtp.canonicalize();
    assert!(smtp.effective_content().carries_text());
    // A relay that moved to another host is another recipient as far as the
    // consent is concerned.
    smtp.config = NotificationSinkConfig::Smtp {
        preset: None,
        host: "smtp.example.com".to_owned(),
        port: 587,
        tls: SmtpTls::StartTls,
        username: "me@qq.com".to_owned(),
    };
    smtp.canonicalize();
    assert_eq!(smtp.content_consent, None);
}

/// #1460: what a candidate holds is bounded before the dwell can carry it, the bound
/// never splits a character, and a cut source says so.
#[test]
fn message_source_is_bounded_on_a_character_boundary_and_flags_a_cut() {
    assert_eq!(bounded_notification_delivery_message_source(""), None);
    let short = bounded_notification_delivery_message_source("short").expect("text");
    assert_eq!(short.text, "short");
    assert!(!short.truncated_at_source);

    let at_cap = "a".repeat(MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS);
    let kept = bounded_notification_delivery_message_source(&at_cap).expect("text");
    assert_eq!(kept.text, at_cap);
    assert!(!kept.truncated_at_source);

    let beyond = format!(
        "{}tail",
        "日".repeat(MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS)
    );
    let bounded = bounded_notification_delivery_message_source(&beyond).expect("text");
    assert_eq!(
        bounded.text.chars().count(),
        MAX_NOTIFICATION_DELIVERY_MESSAGE_SOURCE_CHARS
    );
    assert!(bounded.truncated_at_source);
    assert!(!format!("{bounded:?}").contains('日'));
}
