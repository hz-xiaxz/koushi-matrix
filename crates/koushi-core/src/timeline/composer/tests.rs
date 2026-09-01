use std::sync::Arc;

use koushi_state::{
    AppAction, AvatarThumbnailState, ComposerDocument, ComposerInline, MentionIntent, MentionTarget,
};

use matrix_sdk::room::reply::EnforceThread;

use matrix_sdk::ruma::events::room::message::{MessageType, ReplyWithinThread};

use matrix_sdk_ui::timeline::{Profile, TimelineDetails};
use tokio::sync::mpsc;

use koushi_protocol::failure::TimelineFailureKind;

use super::super::item_projection::{
    attachment_reply_for_key, reply_enforce_thread_for_key, timeline_sender_avatar_from_profile,
    timeline_sender_label_from_profile,
};
use super::super::navigation::TimelineActorGenerationGate;
use super::super::outbound_send::{SubmissionAdmissionLedger, deliver_submission_terminal_action};
use super::super::test_support::{room_key, thread_key};
use super::{
    build_room_message_content_from_composer_body,
    build_room_message_content_from_composer_body_with_options,
    build_room_message_content_from_composer_document,
};

#[tokio::test]
async fn composer_terminals_survive_replacement_during_reducer_capacity_wait() {
    use koushi_state::{ComposerSubmissionTarget, ComposerSubmissionTerminalOutcome, SubmissionId};

    for (label, outcome) in [
        ("success", ComposerSubmissionTerminalOutcome::Succeeded),
        (
            "failure",
            ComposerSubmissionTerminalOutcome::Failed {
                message: "send failed".to_owned(),
            },
        ),
        ("cancel", ComposerSubmissionTerminalOutcome::Cancelled),
    ] {
        let key = room_key();
        let submission_id = SubmissionId::new(format!("{label}-submission"));
        let mut ledger = SubmissionAdmissionLedger::default();
        ledger.accept(submission_id.clone(), key.clone(), format!("{label}-txn"));
        let (action_tx, mut action_rx) = mpsc::channel(1);
        action_tx
            .send(vec![AppAction::ThreadRootProjectionsCleared {
                room_id: "!occupied:test".to_owned(),
            }])
            .await
            .expect("fill reducer channel");
        let delivery = tokio::spawn({
            let action_tx = action_tx.clone();
            let submission_id = submission_id.clone();
            let outcome = outcome.clone();
            async move {
                deliver_submission_terminal_action(
                    &action_tx,
                    AppAction::ComposerSubmissionSettled {
                        submission_id,
                        transaction_id: format!("{label}-txn"),
                        target: ComposerSubmissionTarget::Main {
                            room_id: "!room:test".to_owned(),
                        },
                        outcome,
                    },
                )
                .await
            }
        });
        tokio::task::yield_now().await;

        let generations = Arc::new(TimelineActorGenerationGate::default());
        let _old = generations.activate_after_quiescence(&key).await.generation;
        let _replacement = generations.activate_after_quiescence(&key).await.generation;
        assert!(
            ledger.active.contains_key(&submission_id),
            "ledger must remain active until reducer delivery"
        );

        let _occupied = action_rx.recv().await.expect("occupied reducer slot");
        assert!(delivery.await.expect("terminal delivery task"));
        let delivered = action_rx.recv().await.expect("terminal reducer action");
        assert!(matches!(
            delivered.as_slice(),
            [AppAction::ComposerSubmissionSettled {
                submission_id: delivered_id,
                ..
            }] if delivered_id == &submission_id
        ));
        ledger.terminal(&submission_id);
        assert!(!ledger.active.contains_key(&submission_id));
        assert!(ledger.get(&submission_id).is_some());
    }
}

#[test]
fn composer_document_builds_body_html_and_mentions_from_one_source() {
    let content = build_room_message_content_from_composer_document(ComposerDocument::new(vec![
        ComposerInline::Text {
            text: "hello ".into(),
        },
        ComposerInline::Mention {
            target: MentionTarget::User {
                user_id: "@alice:example.test".into(),
                display_label: "Same Name".into(),
            },
            display_label: "Same Name".into(),
        },
    ]))
    .expect("content");

    let MessageType::Text(text) = &content.msgtype else {
        panic!("expected text content")
    };
    assert_eq!(text.body, "hello @Same Name");
    assert_eq!(
        text.formatted
            .as_ref()
            .map(|formatted| formatted.body.as_str()),
        Some("hello <a href=\"https://matrix.to/#/%40alice%3Aexample.test\">@Same Name</a>")
    );
    assert_eq!(
        content
            .mentions
            .expect("mentions")
            .user_ids
            .iter()
            .next()
            .expect("user")
            .as_str(),
        "@alice:example.test"
    );
}

#[test]
fn composer_core_builds_markdown_send_content_with_mentions() {
    let content = build_room_message_content_from_composer_body(
        "hello **Alice**",
        MentionIntent {
            targets: vec![MentionTarget::User {
                user_id: "@alice:example.test".to_owned(),
                display_label: "Alice".to_owned(),
            }],
        },
    )
    .expect("content");

    match &content.msgtype {
        MessageType::Text(text) => {
            assert_eq!(text.body, "hello **Alice**");
            assert_eq!(
                text.formatted
                    .as_ref()
                    .map(|formatted| formatted.body.as_str()),
                Some("hello <strong>Alice</strong>")
            );
        }
        other => panic!("expected text content, got {other:?}"),
    }

    let mentions = content.mentions.expect("mentions");
    assert!(
        mentions
            .user_ids
            .iter()
            .any(|user_id| user_id.as_str() == "@alice:example.test")
    );
}

#[test]
fn composer_core_builds_me_slash_command_as_emote_content() {
    let content = build_room_message_content_from_composer_body(
        "/me waves **hello**",
        MentionIntent::default(),
    )
    .expect("content");

    match &content.msgtype {
        MessageType::Emote(emote) => {
            assert_eq!(emote.body, "waves **hello**");
            assert_eq!(
                emote
                    .formatted
                    .as_ref()
                    .map(|formatted| formatted.body.as_str()),
                Some("waves <strong>hello</strong>")
            );
        }
        other => panic!("expected emote content, got {other:?}"),
    }
}

#[test]
fn composer_core_builds_spoiler_markdown_as_formatted_body() {
    let content = build_room_message_content_from_composer_body(
        "keep ||secret|| hidden",
        MentionIntent::default(),
    )
    .expect("content");

    match &content.msgtype {
        MessageType::Text(text) => {
            assert_eq!(text.body, "keep ||secret|| hidden");
            assert_eq!(
                text.formatted
                    .as_ref()
                    .map(|formatted| formatted.body.as_str()),
                Some("keep <span data-mx-spoiler>secret</span> hidden")
            );
        }
        other => panic!("expected text content, got {other:?}"),
    }
}

#[test]
fn composer_core_builds_math_markdown_as_matrix_math_html() {
    let content =
        build_room_message_content_from_composer_body("Energy $E=mc^2$", MentionIntent::default())
            .expect("content");

    match &content.msgtype {
        MessageType::Text(text) => {
            assert_eq!(text.body, "Energy $E=mc^2$");
            assert_eq!(
                text.formatted
                    .as_ref()
                    .map(|formatted| formatted.body.as_str()),
                Some("Energy <span data-mx-maths=\"E=mc^2\">E=mc^2</span>")
            );
        }
        other => panic!("expected text content, got {other:?}"),
    }
}

#[test]
fn composer_core_respects_math_mode_off_for_sent_content() {
    let content = build_room_message_content_from_composer_body_with_options(
        "Energy $E=mc^2$",
        MentionIntent::default(),
        koushi_state::ComposerFormattingOptions { math_mode: false },
    )
    .expect("content");

    match &content.msgtype {
        MessageType::Text(text) => {
            assert_eq!(text.body, "Energy $E=mc^2$");
            assert!(text.formatted.is_none());
        }
        other => panic!("expected text content, got {other:?}"),
    }
}

#[test]
fn sender_profile_projects_display_name_and_avatar_mxc() {
    let profile = TimelineDetails::Ready(Profile {
        display_name: Some("kamohara".to_owned()),
        display_name_ambiguous: false,
        avatar_url: Some(matrix_sdk::ruma::OwnedMxcUri::from(
            "mxc://matrix.org/avatar".to_owned(),
        )),
    });

    assert_eq!(
        timeline_sender_label_from_profile(&profile),
        Some("kamohara".to_owned())
    );
    let avatar = timeline_sender_avatar_from_profile(&profile).expect("avatar");
    assert_eq!(avatar.mxc_uri, "mxc://matrix.org/avatar");
    assert_eq!(avatar.thumbnail, AvatarThumbnailState::NotRequested);
}

#[test]
fn composer_core_sends_unknown_slash_text_literally() {
    // Issue #450: unknown leading-slash text is ordinary content.
    for body in ["/shrug nope", "/usr/local/bin", "/not-a-command", "/ 文章"] {
        let content = build_room_message_content_from_composer_body(body, MentionIntent::default())
            .expect("ordinary leading-slash text must send");
        assert_eq!(content.body(), body);
    }
}

#[test]
fn composer_core_rejects_recognized_unavailable_commands_locally() {
    // Issue #450: /me is sent (emote); /join and /invite are recognized
    // but unavailable on this surface and rejected before any SDK send.
    assert!(
        build_room_message_content_from_composer_body("/me waves", MentionIntent::default(),)
            .is_ok()
    );
    for body in [
        "/join #room:example.invalid",
        "/invite @alice:example.invalid",
    ] {
        assert_eq!(
            build_room_message_content_from_composer_body(body, MentionIntent::default())
                .expect_err("recognized-but-unavailable command should fail before SDK send"),
            TimelineFailureKind::UnsupportedSlashCommand
        );
    }
}

#[test]
fn thread_composer_sends_regular_thread_messages_for_element_compatibility() {
    assert_eq!(
        reply_enforce_thread_for_key(&thread_key()),
        EnforceThread::Threaded(ReplyWithinThread::No)
    );
}

#[test]
fn thread_media_uses_the_same_regular_thread_relation() {
    let reply = attachment_reply_for_key(&thread_key()).expect("thread media relation");
    assert_eq!(
        reply.enforce_thread,
        EnforceThread::Threaded(ReplyWithinThread::No)
    );
    assert_eq!(reply.event_id.as_str(), "$root:test");
    assert!(attachment_reply_for_key(&room_key()).is_none());
}
