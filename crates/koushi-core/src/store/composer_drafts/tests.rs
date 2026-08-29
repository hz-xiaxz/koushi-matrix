use super::*;
use koushi_state::{ComposerInline, MentionTarget};

const LARGE_LEGACY_REVISION: u64 = 9_007_199_254_740_993;

#[test]
fn composer_draft_payload_pre_293_defaults_content_revision_and_clear_token_to_zero() {
    let legacy = br#"{
            "rooms":{"room-legacy":"room body"},
            "threads":{"room-legacy":{"root-legacy":"thread body"}}
        }"#;

    let mut decoded = decode_payload_json(legacy).expect("decode pre-#293 payload");
    let room = decoded.composer_for_room("room-legacy");
    assert_eq!(room.draft, "room body");
    assert!(room.draft_revision.is_zero());
    assert!(room.last_accepted_clear_revision.is_zero());
    let thread = decoded.composer_for_thread("room-legacy", "root-legacy");
    assert_eq!(thread.draft, "thread body");
    assert!(thread.draft_revision.is_zero());
    assert!(thread.last_accepted_clear_revision.is_zero());

    assert!(
        decoded
            .apply_room_draft("room-legacy".to_owned(), "mutated".to_owned(), 1.into())
            .expect("checked mutation")
    );
    let encoded = encode_payload_json(&persisted_projection(
        &decoded,
        &ComposerDraftProtection::default(),
    ))
    .expect("encode v2");
    let reloaded = decode_payload_json(&encoded).expect("reload v2");
    assert_eq!(
        reloaded
            .rooms
            .get("room-legacy")
            .map(ComposerDocument::plain_body),
        Some("mutated".to_owned())
    );
    assert_eq!(reloaded.room_revision("room-legacy"), 1.into());
    assert!(
        reloaded
            .composer_for_room("room-legacy")
            .last_accepted_clear_revision
            .is_zero()
    );
}

#[test]
fn composer_draft_payload_issue_293_numeric_u64_migrates_losslessly_to_strings() {
    let legacy = format!(
        r#"{{
                "rooms":{{"room-large":"room body"}},
                "threads":{{"room-large":{{"root-large":"thread body"}}}},
                "room_revisions":{{"room-large":{LARGE_LEGACY_REVISION}}},
                "thread_revisions":{{"room-large":{{"root-large":{LARGE_LEGACY_REVISION}}}}}
            }}"#
    );

    let decoded = decode_payload_json(legacy.as_bytes()).expect("decode #293 payload");
    let encoded = encode_payload_json(&persisted_projection(
        &decoded,
        &ComposerDraftProtection::default(),
    ))
    .expect("encode v2");
    let encoded: serde_json::Value = serde_json::from_slice(&encoded).expect("parse encoded v2");

    assert_eq!(
        encoded["rooms"]["room-large"]["revision"],
        serde_json::json!("9007199254740993")
    );
    assert_eq!(
        encoded["threads"]["room-large"]["root-large"]["revision"],
        serde_json::json!("9007199254740993")
    );
}

#[test]
fn composer_draft_payload_legacy_clear_watermarks_migrate_losslessly() {
    let legacy = format!(
        r#"{{
                "room_revisions":{{"room-cleared":{LARGE_LEGACY_REVISION}}},
                "thread_revisions":{{"room-cleared":{{"root-cleared":{LARGE_LEGACY_REVISION}}}}},
                "room_last_accepted_clear_revisions":{{"room-cleared":{LARGE_LEGACY_REVISION}}},
                "thread_last_accepted_clear_revisions":{{"room-cleared":{{"root-cleared":{LARGE_LEGACY_REVISION}}}}},
                "quiescent_room_lru":["room-cleared"],
                "quiescent_thread_lru":[["room-cleared","root-cleared"]]
            }}"#
    );

    let decoded = decode_payload_json(legacy.as_bytes()).expect("decode causal legacy payload");
    assert_eq!(
        decoded
            .composer_for_room("room-cleared")
            .last_accepted_clear_revision,
        ComposerDraftRevision::from_u64(LARGE_LEGACY_REVISION)
    );
    assert_eq!(
        decoded
            .composer_for_thread("room-cleared", "root-cleared")
            .last_accepted_clear_revision,
        ComposerDraftRevision::from_u64(LARGE_LEGACY_REVISION)
    );

    let encoded = encode_payload_json(&persisted_projection(
        &decoded,
        &ComposerDraftProtection::default(),
    ))
    .expect("encode migrated v2");
    let encoded: serde_json::Value = serde_json::from_slice(&encoded).expect("parse migrated v2");
    assert_eq!(
        encoded["rooms"]["room-cleared"]["last_accepted_clear_revision"],
        serde_json::json!("9007199254740993")
    );
    assert_eq!(
        encoded["threads"]["room-cleared"]["root-cleared"]["last_accepted_clear_revision"],
        serde_json::json!("9007199254740993")
    );
}

#[test]
fn composer_draft_payload_legacy_lru_preserves_nonlexical_order_and_rejects_invalid_order() {
    let legacy = br#"{
            "room_revisions":{"z-oldest":1,"a-newer":1,"middle-missing-order":1},
            "thread_revisions":{"z-room":{"z-root":1,"a-root":1,"middle-root":1}},
            "quiescent_room_lru":["z-oldest","a-newer"],
            "quiescent_thread_lru":[["z-room","z-root"],["z-room","a-root"]]
        }"#;

    let decoded = decode_payload_json(legacy).expect("decode ordered legacy payload");
    let projection = persisted_projection(&decoded, &ComposerDraftProtection::default());
    assert_eq!(
        projection.quiescent_room_order,
        vec!["z-oldest", "a-newer", "middle-missing-order"]
    );
    assert_eq!(
        projection.quiescent_thread_order,
        vec![
            ("z-room".to_owned(), "z-root".to_owned()),
            ("z-room".to_owned(), "a-root".to_owned()),
            ("z-room".to_owned(), "middle-root".to_owned()),
        ]
    );

    let invalid = [
        br#"{
                "room_revisions":{"room":1},
                "quiescent_room_lru":["room","room"]
            }"#
        .as_slice(),
        br#"{
                "room_revisions":{"room":1},
                "quiescent_room_lru":["unknown"]
            }"#
        .as_slice(),
        br#"{
                "threads":{"room":{"root":"body"}},
                "thread_revisions":{"room":{"root":1}},
                "quiescent_thread_lru":[["room","root"]]
            }"#
        .as_slice(),
        br#"{
                "room_revisionz":{"room":1}
            }"#
        .as_slice(),
    ];
    for payload in invalid {
        assert_eq!(
            decode_payload_json(payload).expect_err("invalid legacy order must fail"),
            ComposerDraftPayloadError::Corrupt
        );
    }
}

#[test]
fn composer_draft_payload_v2_migrates_strings_as_text_without_mentions() {
    let payload = br#"{
            "schema_version":2,
            "rooms":{"room":{"content":"@Same Name","revision":"1","last_accepted_clear_revision":"0"}},
            "threads":{},"quiescent_room_order":[],"quiescent_thread_order":[],
            "protected_empty_rooms":[],"protected_empty_threads":[]
        }"#;

    let drafts = decode_payload_json(payload).expect("migrate v2 document");
    let composer = drafts.composer_for_room("room");
    assert_eq!(composer.draft, "@Same Name");
    assert_eq!(
        composer.document,
        ComposerDocument::from_plain_text("@Same Name")
    );
    assert!(composer.document.mention_intent().targets.is_empty());
}

#[test]
fn composer_draft_payload_v3_round_trips_structured_mention_identity() {
    let target = MentionTarget::User {
        user_id: "@alice:example.invalid".to_owned(),
        display_label: "Same Name".to_owned(),
    };
    let document = ComposerDocument::new(vec![
        ComposerInline::Text {
            text: "hello ".to_owned(),
        },
        ComposerInline::Mention {
            target: target.clone(),
            display_label: "Same Name".to_owned(),
        },
    ]);
    let mut drafts = ComposerDraftStore::default();
    drafts.set_room_draft("room".to_owned(), document.clone());

    let encoded = encode_payload_json(&persisted_projection(
        &drafts,
        &ComposerDraftProtection::default(),
    ))
    .expect("encode v3");
    let json: serde_json::Value = serde_json::from_slice(&encoded).expect("parse v3");
    assert_eq!(json["schema_version"], serde_json::json!(3));

    let reloaded = decode_payload_json(&encoded).expect("reload v3");
    assert_eq!(reloaded.composer_for_room("room").document, document);
    assert_eq!(
        reloaded
            .composer_for_room("room")
            .document
            .mention_intent()
            .targets,
        vec![target]
    );
}

#[test]
fn composer_draft_payload_v3_round_trips_bounded_empty_documents() {
    let mut drafts = ComposerDraftStore::default();
    for index in 0..(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT + 2) {
        let room_id = format!("empty-room-{index:03}");
        drafts
            .rooms
            .insert(room_id.clone(), ComposerDocument::default());
        drafts.room_revisions.insert(room_id, 1.into());
    }

    let encoded = encode_payload_json(&persisted_projection(
        &drafts,
        &ComposerDraftProtection::default(),
    ))
    .expect("encode bounded v3");
    let decoded = decode_payload_json(&encoded).expect("self-encoded v3 must decode");

    assert_eq!(
        decoded.room_revisions.len(),
        koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT
    );
    assert!(!decoded.room_revisions.contains_key("empty-room-000"));
    assert!(!decoded.room_revisions.contains_key("empty-room-001"));
    assert!(decoded.room_revisions.contains_key("empty-room-002"));
}

#[test]
fn composer_draft_payload_rejects_noncanonical_overflow_and_duplicate_order_entries() {
    let cases = [
            br#"{
                "schema_version":3,
                "rooms":{"room":{"content":null,"revision":"01","last_accepted_clear_revision":"0"}},
                "threads":{},"quiescent_room_order":["room"],"quiescent_thread_order":[],
                "protected_empty_rooms":[],"protected_empty_threads":[]
            }"#
        .as_slice(),
            br#"{
                "schema_version":3,
                "rooms":{"room":{"content":null,"revision":"340282366920938463463374607431768211456","last_accepted_clear_revision":"0"}},
                "threads":{},"quiescent_room_order":["room"],"quiescent_thread_order":[],
                "protected_empty_rooms":[],"protected_empty_threads":[]
            }"#
        .as_slice(),
            br#"{
                "schema_version":3,
                "rooms":{"room":{"content":null,"revision":"1","last_accepted_clear_revision":"0"}},
                "threads":{},"quiescent_room_order":["room","room"],"quiescent_thread_order":[],
                "protected_empty_rooms":[],"protected_empty_threads":[]
            }"#
        .as_slice(),
            br#"{
                "schema_version":3,
                "rooms":{},
                "threads":{"room":{"root":{"content":null,"revision":"1","last_accepted_clear_revision":"0"}}},
                "quiescent_room_order":[],"quiescent_thread_order":[["room","root"],["room","root"]],
                "protected_empty_rooms":[],"protected_empty_threads":[]
            }"#
        .as_slice(),
    ];

    for payload in cases {
        let error = decode_payload_json(payload).expect_err("invalid v3 must be rejected");
        assert_eq!(error, ComposerDraftPayloadError::Corrupt);
        let debug = format!("{error:?}");
        assert_eq!(debug, "Corrupt");
        assert!(!debug.contains("room"));
        assert!(!debug.contains("340282366920938463463374607431768211456"));
    }
}
