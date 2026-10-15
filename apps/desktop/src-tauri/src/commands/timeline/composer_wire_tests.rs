use super::*;

#[test]
fn composer_wire_tokens_accept_only_canonical_nonzero_decimal_u64() {
    let (generation, lease) =
        parse_composer_wire_tokens("7", "9").expect("canonical composer tokens");
    assert_eq!(generation.to_wire_string(), "7");
    assert_eq!(lease.to_wire_string(), "9");

    for (generation, lease) in [("0", "1"), ("01", "1"), ("1", "00")] {
        assert!(
            parse_composer_wire_tokens(generation, lease).is_err(),
            "noncanonical composer token pair must be rejected"
        );
    }
    assert!(parse_composer_wire_tokens("18446744073709551616", "1").is_err());
}

/// #1426: a message typed in the thread pane is a plain thread message, so the
/// builder must submit text on the thread key. `SubmitReply` with the root as
/// the target made every thread message a genuine in-thread reply that quoted
/// the root and mentioned its sender.
#[test]
fn thread_composer_submission_builds_plain_thread_text() {
    let command = build_submit_thread_text_command(
        koushi_protocol::RequestId {
            connection_id: koushi_protocol::RuntimeConnectionId(7),
            sequence: 21,
        },
        koushi_protocol::SessionKeyId {
            homeserver: "https://example.invalid".to_owned(),
            user_id: "@alice:example.invalid".to_owned(),
            device_id: "DEVICE".to_owned(),
        },
        SubmissionId::new("submission"),
        AccountKey("@alice:example.invalid".to_owned()),
        "!room:example.invalid".to_owned(),
        "$root:example.invalid".to_owned(),
        "desktop-21".to_owned(),
        ComposerDocument::from_plain_text("thread message"),
        3.into(),
    );

    match command {
        Some(CoreCommand::Timeline(TimelineCommand::SubmitText { key, .. })) => {
            assert_eq!(
                key.kind,
                TimelineKind::Thread {
                    room_id: "!room:example.invalid".to_owned(),
                    root_event_id: "$root:example.invalid".to_owned(),
                }
            );
        }
        other => panic!("thread composer must submit plain text on the thread key, got {other:?}"),
    }
}
