use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[cfg(any(test, feature = "test-hooks"))]
use super::ComposerDraftIoProbe;
use super::{
    COMPOSER_DRAFTS_FILE_MAGIC, COMPOSER_DRAFTS_NONCE_LEN, CoreFailure, StoreActor,
    atomic_replace_file, decode_composer_draft_payload_json, encode_composer_draft_payload_json,
};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use koushi_key::{LocalUnlockSecret, SessionKeyId};

use koushi_state::{
    ComposerDocument, ComposerDraftPersistenceEntry, ComposerDraftPersistenceProjection,
    ComposerDraftProtection, ComposerDraftRevision, ComposerDraftStore, ComposerTarget,
};
use serde::{Deserialize, Serialize};

const COMPOSER_DRAFT_PAYLOAD_SCHEMA_VERSION: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ComposerDraftPayloadError {
    Corrupt,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedComposerDraftStoreV3 {
    schema_version: u8,
    rooms: BTreeMap<String, PersistedComposerDraftEntry>,
    threads: BTreeMap<String, BTreeMap<String, PersistedComposerDraftEntry>>,
    quiescent_room_order: Vec<String>,
    quiescent_thread_order: Vec<(String, String)>,
    protected_empty_rooms: Vec<String>,
    protected_empty_threads: Vec<(String, String)>,
}

impl PersistedComposerDraftStoreV3 {
    pub(crate) fn is_empty(&self) -> bool {
        self.rooms.is_empty() && self.threads.is_empty()
    }

    pub(crate) fn targets(&self) -> BTreeSet<ComposerTarget> {
        self.rooms
            .keys()
            .cloned()
            .map(|room_id| ComposerTarget::Main { room_id })
            .chain(self.threads.iter().flat_map(|(room_id, room_threads)| {
                room_threads
                    .keys()
                    .cloned()
                    .map(|root_event_id| ComposerTarget::Thread {
                        room_id: room_id.clone(),
                        root_event_id,
                    })
            }))
            .collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedComposerDraftEntry {
    content: Option<ComposerDocument>,
    revision: ComposerDraftRevision,
    last_accepted_clear_revision: ComposerDraftRevision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedComposerDraftStoreV2 {
    schema_version: u8,
    rooms: BTreeMap<String, PersistedComposerDraftEntryV2>,
    threads: BTreeMap<String, BTreeMap<String, PersistedComposerDraftEntryV2>>,
    quiescent_room_order: Vec<String>,
    quiescent_thread_order: Vec<(String, String)>,
    protected_empty_rooms: Vec<String>,
    protected_empty_threads: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedComposerDraftEntryV2 {
    content: Option<String>,
    revision: ComposerDraftRevision,
    last_accepted_clear_revision: ComposerDraftRevision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyComposerDraftStoreV1 {
    #[serde(default)]
    rooms: BTreeMap<String, String>,
    #[serde(default)]
    threads: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    room_revisions: BTreeMap<String, u64>,
    #[serde(default)]
    thread_revisions: BTreeMap<String, BTreeMap<String, u64>>,
    #[serde(default)]
    room_last_accepted_clear_revisions: BTreeMap<String, u64>,
    #[serde(default)]
    thread_last_accepted_clear_revisions: BTreeMap<String, BTreeMap<String, u64>>,
    #[serde(default)]
    quiescent_room_lru: Vec<String>,
    #[serde(default)]
    quiescent_thread_lru: Vec<(String, String)>,
}

pub(crate) fn persisted_projection(
    drafts: &ComposerDraftStore,
    protection: &ComposerDraftProtection,
) -> PersistedComposerDraftStoreV3 {
    let projection = drafts.persisted_projection(protection);
    PersistedComposerDraftStoreV3 {
        schema_version: COMPOSER_DRAFT_PAYLOAD_SCHEMA_VERSION,
        rooms: projection
            .rooms
            .into_iter()
            .map(|(room_id, entry)| (room_id, entry.into()))
            .collect(),
        threads: projection
            .threads
            .into_iter()
            .map(|(room_id, room_threads)| {
                (
                    room_id,
                    room_threads
                        .into_iter()
                        .map(|(root_event_id, entry)| (root_event_id, entry.into()))
                        .collect(),
                )
            })
            .collect(),
        quiescent_room_order: projection.quiescent_room_order,
        quiescent_thread_order: projection.quiescent_thread_order,
        protected_empty_rooms: projection.protected_empty_rooms,
        protected_empty_threads: projection.protected_empty_threads,
    }
}

pub(crate) fn encode_payload_json(
    drafts: &PersistedComposerDraftStoreV3,
) -> Result<Vec<u8>, ComposerDraftPayloadError> {
    serde_json::to_vec(drafts).map_err(|_| ComposerDraftPayloadError::Corrupt)
}

pub(crate) fn decode_payload_json(
    payload: &[u8],
) -> Result<ComposerDraftStore, ComposerDraftPayloadError> {
    let value = serde_json::from_slice::<serde_json::Value>(payload).map_err(|_| corrupt())?;
    if let Some(schema_version) = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    {
        let projection = match schema_version {
            3 => serde_json::from_value::<PersistedComposerDraftStoreV3>(value)
                .map(Into::into)
                .map_err(|_| corrupt())?,
            2 => serde_json::from_value::<PersistedComposerDraftStoreV2>(value)
                .map(Into::into)
                .map_err(|_| corrupt())?,
            _ => return Err(corrupt()),
        };
        ComposerDraftStore::from_persisted_projection(projection).map_err(|_| corrupt())
    } else {
        let legacy =
            serde_json::from_value::<LegacyComposerDraftStoreV1>(value).map_err(|_| corrupt())?;
        let projection = ComposerDraftPersistenceProjection::try_from(legacy)?;
        ComposerDraftStore::from_persisted_projection(projection).map_err(|_| corrupt())
    }
}

fn corrupt() -> ComposerDraftPayloadError {
    ComposerDraftPayloadError::Corrupt
}

impl From<ComposerDraftPersistenceEntry> for PersistedComposerDraftEntry {
    fn from(entry: ComposerDraftPersistenceEntry) -> Self {
        Self {
            content: entry.content,
            revision: entry.revision,
            last_accepted_clear_revision: entry.last_accepted_clear_revision,
        }
    }
}

impl From<PersistedComposerDraftEntry> for ComposerDraftPersistenceEntry {
    fn from(entry: PersistedComposerDraftEntry) -> Self {
        Self {
            content: entry.content,
            revision: entry.revision,
            last_accepted_clear_revision: entry.last_accepted_clear_revision,
        }
    }
}

impl From<PersistedComposerDraftStoreV3> for ComposerDraftPersistenceProjection {
    fn from(persisted: PersistedComposerDraftStoreV3) -> Self {
        Self {
            rooms: persisted
                .rooms
                .into_iter()
                .map(|(room_id, entry)| (room_id, entry.into()))
                .collect(),
            threads: persisted
                .threads
                .into_iter()
                .map(|(room_id, room_threads)| {
                    (
                        room_id,
                        room_threads
                            .into_iter()
                            .map(|(root_event_id, entry)| (root_event_id, entry.into()))
                            .collect(),
                    )
                })
                .collect(),
            quiescent_room_order: persisted.quiescent_room_order,
            quiescent_thread_order: persisted.quiescent_thread_order,
            protected_empty_rooms: persisted.protected_empty_rooms,
            protected_empty_threads: persisted.protected_empty_threads,
        }
    }
}

impl From<PersistedComposerDraftStoreV2> for ComposerDraftPersistenceProjection {
    fn from(persisted: PersistedComposerDraftStoreV2) -> Self {
        debug_assert_eq!(persisted.schema_version, 2);
        let convert = |entry: PersistedComposerDraftEntryV2| ComposerDraftPersistenceEntry {
            content: entry.content.map(ComposerDocument::from_plain_text),
            revision: entry.revision,
            last_accepted_clear_revision: entry.last_accepted_clear_revision,
        };
        Self {
            rooms: persisted
                .rooms
                .into_iter()
                .map(|(room_id, entry)| (room_id, convert(entry)))
                .collect(),
            threads: persisted
                .threads
                .into_iter()
                .map(|(room_id, room_threads)| {
                    (
                        room_id,
                        room_threads
                            .into_iter()
                            .map(|(root_event_id, entry)| (root_event_id, convert(entry)))
                            .collect(),
                    )
                })
                .collect(),
            quiescent_room_order: persisted.quiescent_room_order,
            quiescent_thread_order: persisted.quiescent_thread_order,
            protected_empty_rooms: persisted.protected_empty_rooms,
            protected_empty_threads: persisted.protected_empty_threads,
        }
    }
}

impl TryFrom<LegacyComposerDraftStoreV1> for ComposerDraftPersistenceProjection {
    type Error = ComposerDraftPayloadError;

    fn try_from(legacy: LegacyComposerDraftStoreV1) -> Result<Self, Self::Error> {
        let room_ids = legacy
            .rooms
            .keys()
            .chain(legacy.room_revisions.keys())
            .chain(legacy.room_last_accepted_clear_revisions.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let rooms = room_ids
            .iter()
            .map(|room_id| {
                (
                    room_id.clone(),
                    ComposerDraftPersistenceEntry {
                        content: legacy
                            .rooms
                            .get(room_id)
                            .filter(|content| !content.is_empty())
                            .cloned()
                            .map(ComposerDocument::from_plain_text),
                        revision: legacy
                            .room_revisions
                            .get(room_id)
                            .copied()
                            .map(ComposerDraftRevision::from_u64)
                            .unwrap_or_default(),
                        last_accepted_clear_revision: legacy
                            .room_last_accepted_clear_revisions
                            .get(room_id)
                            .copied()
                            .map(ComposerDraftRevision::from_u64)
                            .unwrap_or_default(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let empty_room_ids = room_ids
            .into_iter()
            .filter(|room_id| {
                rooms
                    .get(room_id)
                    .is_some_and(|entry| entry.content.is_none())
            })
            .collect::<BTreeSet<_>>();
        let quiescent_room_order = merge_legacy_order(legacy.quiescent_room_lru, &empty_room_ids)?;

        let thread_targets = legacy
            .threads
            .iter()
            .flat_map(|(room_id, threads)| {
                threads
                    .keys()
                    .map(|root_event_id| (room_id.clone(), root_event_id.clone()))
            })
            .chain(
                legacy
                    .thread_revisions
                    .iter()
                    .flat_map(|(room_id, revisions)| {
                        revisions
                            .keys()
                            .map(|root_event_id| (room_id.clone(), root_event_id.clone()))
                    }),
            )
            .chain(legacy.thread_last_accepted_clear_revisions.iter().flat_map(
                |(room_id, revisions)| {
                    revisions
                        .keys()
                        .map(|root_event_id| (room_id.clone(), root_event_id.clone()))
                },
            ))
            .collect::<std::collections::BTreeSet<_>>();
        let mut threads = BTreeMap::<String, BTreeMap<String, _>>::new();
        let mut empty_thread_targets = BTreeSet::new();
        for (room_id, root_event_id) in thread_targets {
            let content = legacy
                .threads
                .get(&room_id)
                .and_then(|room_threads| room_threads.get(&root_event_id))
                .filter(|content| !content.is_empty())
                .cloned()
                .map(ComposerDocument::from_plain_text);
            if content.is_none() {
                empty_thread_targets.insert((room_id.clone(), root_event_id.clone()));
            }
            let revision = legacy
                .thread_revisions
                .get(&room_id)
                .and_then(|room_threads| room_threads.get(&root_event_id))
                .copied()
                .map(ComposerDraftRevision::from_u64)
                .unwrap_or_default();
            let last_accepted_clear_revision = legacy
                .thread_last_accepted_clear_revisions
                .get(&room_id)
                .and_then(|room_threads| room_threads.get(&root_event_id))
                .copied()
                .map(ComposerDraftRevision::from_u64)
                .unwrap_or_default();
            threads.entry(room_id).or_default().insert(
                root_event_id,
                ComposerDraftPersistenceEntry {
                    content,
                    revision,
                    last_accepted_clear_revision,
                },
            );
        }
        let quiescent_thread_order =
            merge_legacy_order(legacy.quiescent_thread_lru, &empty_thread_targets)?;

        Ok(Self {
            rooms,
            threads,
            quiescent_room_order,
            quiescent_thread_order,
            protected_empty_rooms: Vec::new(),
            protected_empty_threads: Vec::new(),
        })
    }
}

fn merge_legacy_order<T: Clone + Ord>(
    mut saved_order: Vec<T>,
    empty_targets: &BTreeSet<T>,
) -> Result<Vec<T>, ComposerDraftPayloadError> {
    let mut seen = BTreeSet::new();
    if saved_order
        .iter()
        .any(|target| !empty_targets.contains(target) || !seen.insert(target.clone()))
    {
        return Err(corrupt());
    }
    saved_order.extend(empty_targets.difference(&seen).cloned());
    Ok(saved_order)
}

#[cfg(test)]
mod tests {
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
        let encoded: serde_json::Value =
            serde_json::from_slice(&encoded).expect("parse encoded v2");

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
        let encoded: serde_json::Value =
            serde_json::from_slice(&encoded).expect("parse migrated v2");
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
}

impl StoreActor {
    pub fn load_composer_drafts(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<ComposerDraftStore, CoreFailure> {
        #[cfg(any(test, feature = "test-hooks"))]
        self.notify_composer_draft_load_started_for_testing();
        let result = (|| {
            let path = self.account_composer_drafts_file(key_id);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ComposerDraftStore::default());
                }
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            };
            decrypt_composer_drafts_payload(&self.load_unlock_secret(key_id)?, &bytes)
        })();
        #[cfg(any(test, feature = "test-hooks"))]
        self.notify_composer_draft_load_completed_for_testing();
        result
    }

    pub(crate) fn save_composer_drafts(
        &self,
        key_id: &SessionKeyId,
        drafts: &PersistedComposerDraftStoreV3,
    ) -> Result<(), CoreFailure> {
        #[cfg(any(test, feature = "test-hooks"))]
        self.wait_for_composer_draft_save_release_for_testing();
        let result = self.save_composer_drafts_inner(key_id, drafts);
        #[cfg(any(test, feature = "test-hooks"))]
        self.notify_composer_draft_save_completed_for_testing();
        result
    }

    fn save_composer_drafts_inner(
        &self,
        key_id: &SessionKeyId,
        drafts: &PersistedComposerDraftStoreV3,
    ) -> Result<(), CoreFailure> {
        let path = self.account_composer_drafts_file(key_id);
        if drafts.is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(_) => return Err(CoreFailure::StoreUnavailable),
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        let payload =
            encrypt_composer_drafts_payload(&self.load_or_create_unlock_secret(key_id)?, drafts)?;
        #[cfg(test)]
        let fail_before_persist = self
            .composer_draft_replace_fault
            .swap(false, std::sync::atomic::Ordering::AcqRel);
        #[cfg(not(test))]
        let fail_before_persist = false;
        atomic_replace_file(&path, &payload, fail_before_persist)
    }

    #[cfg(test)]
    fn fail_next_composer_draft_replace_for_testing(&self) {
        self.composer_draft_replace_fault
            .store(true, std::sync::atomic::Ordering::Release);
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn install_composer_draft_io_probe(
        &self,
        save_started: tokio::sync::oneshot::Sender<()>,
        save_release: std::sync::mpsc::Receiver<()>,
        save_completed: tokio::sync::oneshot::Sender<()>,
        load_started: tokio::sync::oneshot::Sender<()>,
        load_completed: tokio::sync::oneshot::Sender<()>,
    ) {
        *self
            .composer_draft_io_probe
            .lock()
            .expect("composer draft I/O probe mutex") = Some(ComposerDraftIoProbe {
            save_started: Some(save_started),
            save_release: Some(save_release),
            save_completed: Some(save_completed),
            load_started: Some(load_started),
            load_completed: Some(load_completed),
        });
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn wait_for_composer_draft_save_release_for_testing(&self) {
        let (started, release) = {
            let mut probe = self
                .composer_draft_io_probe
                .lock()
                .expect("composer draft I/O probe mutex");
            let Some(probe) = probe.as_mut() else {
                return;
            };
            (probe.save_started.take(), probe.save_release.take())
        };
        if let Some(started) = started {
            let _ = started.send(());
        }
        if let Some(release) = release {
            let _ = release.recv();
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn notify_composer_draft_save_completed_for_testing(&self) {
        let completed = self
            .composer_draft_io_probe
            .lock()
            .expect("composer draft I/O probe mutex")
            .as_mut()
            .and_then(|probe| probe.save_completed.take());
        if let Some(completed) = completed {
            let _ = completed.send(());
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn notify_composer_draft_load_started_for_testing(&self) {
        let started = self
            .composer_draft_io_probe
            .lock()
            .expect("composer draft I/O probe mutex")
            .as_mut()
            .and_then(|probe| probe.load_started.take());
        if let Some(started) = started {
            let _ = started.send(());
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn notify_composer_draft_load_completed_for_testing(&self) {
        let completed = self
            .composer_draft_io_probe
            .lock()
            .expect("composer draft I/O probe mutex")
            .as_mut()
            .and_then(|probe| probe.load_completed.take());
        if let Some(completed) = completed {
            let _ = completed.send(());
        }
    }

    fn account_composer_drafts_file(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("composer-drafts")
            .join("drafts.v1.enc")
    }
}

fn encrypt_composer_drafts_payload(
    secret: &LocalUnlockSecret,
    drafts: &PersistedComposerDraftStoreV3,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext =
        encode_composer_draft_payload_json(drafts).map_err(|_| CoreFailure::StoreUnavailable)?;
    encrypt_composer_drafts_plaintext(secret, &plaintext)
}

fn encrypt_composer_drafts_plaintext(
    secret: &LocalUnlockSecret,
    plaintext: &[u8],
) -> Result<Vec<u8>, CoreFailure> {
    let key = secret.derive_composer_drafts_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0_u8; COMPOSER_DRAFTS_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    let mut payload = Vec::with_capacity(
        COMPOSER_DRAFTS_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN + ciphertext.len(),
    );
    payload.extend_from_slice(COMPOSER_DRAFTS_FILE_MAGIC);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

#[cfg(test)]
fn encrypt_composer_drafts_fixture_payload(
    secret: &LocalUnlockSecret,
    plaintext: &[u8],
) -> Result<Vec<u8>, CoreFailure> {
    encrypt_composer_drafts_plaintext(secret, plaintext)
}

fn decrypt_composer_drafts_payload(
    secret: &LocalUnlockSecret,
    payload: &[u8],
) -> Result<ComposerDraftStore, CoreFailure> {
    let header_len = COMPOSER_DRAFTS_FILE_MAGIC.len() + COMPOSER_DRAFTS_NONCE_LEN;
    if payload.len() < header_len || !payload.starts_with(COMPOSER_DRAFTS_FILE_MAGIC) {
        return Err(CoreFailure::StoreUnavailable);
    }
    let nonce_start = COMPOSER_DRAFTS_FILE_MAGIC.len();
    let nonce_end = nonce_start + COMPOSER_DRAFTS_NONCE_LEN;
    let nonce = Nonce::from_slice(&payload[nonce_start..nonce_end]);
    let key = secret.derive_composer_drafts_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_bytes()));
    let plaintext = cipher
        .decrypt(nonce, &payload[nonce_end..])
        .map_err(|_| CoreFailure::StoreUnavailable)?;
    decode_composer_draft_payload_json(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)
}

#[cfg(test)]
mod store_tests {
    use super::super::test_support::{file_store_actor, make_key_id};
    use super::super::*;
    use super::{
        CoreFailure, PersistedComposerDraftStoreV3, encrypt_composer_drafts_fixture_payload,
        persisted_projection,
    };
    use koushi_state::ComposerDraftStore;
    use tempfile::tempdir;

    fn persisted_composer_drafts(drafts: &ComposerDraftStore) -> PersistedComposerDraftStoreV3 {
        persisted_projection(drafts, &koushi_state::ComposerDraftProtection::default())
    }

    #[test]
    fn composer_drafts_are_encrypted_and_reject_corruption() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let plaintext = "secret draft body";
        let mut drafts = ComposerDraftStore::default();
        drafts.set_room_draft("!room:test.example.com".to_owned(), plaintext.to_owned());
        assert!(
            drafts
                .apply_room_draft(
                    "!sent:test.example.com".to_owned(),
                    "accepted body".to_owned(),
                    7.into(),
                )
                .expect("room draft revision should apply")
        );
        assert_eq!(
            drafts
                .advance_room_revision("!sent:test.example.com", 7.into())
                .expect("room acceptance should advance"),
            8.into()
        );
        assert!(
            drafts
                .apply_thread_draft(
                    "!room:test.example.com".to_owned(),
                    "$root:test.example.com".to_owned(),
                    "thread accepted body".to_owned(),
                    11.into(),
                )
                .expect("thread draft revision should apply")
        );
        assert_eq!(
            drafts
                .advance_thread_revision(
                    "!room:test.example.com",
                    "$root:test.example.com",
                    11.into(),
                )
                .expect("thread acceptance should advance"),
            12.into()
        );

        actor
            .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
            .expect("save encrypted drafts");

        let path = actor.account_composer_drafts_file(&key_id);
        let bytes = std::fs::read(&path).expect("read encrypted drafts");
        assert!(
            !bytes
                .windows(plaintext.len())
                .any(|window| window == plaintext.as_bytes())
        );

        let loaded = actor
            .load_composer_drafts(&key_id)
            .expect("load encrypted drafts");
        assert_eq!(
            loaded
                .rooms
                .get("!room:test.example.com")
                .map(koushi_state::ComposerDocument::plain_body),
            Some(plaintext.to_owned())
        );
        assert_eq!(loaded.room_revision("!sent:test.example.com"), 8.into());
        assert!(!loaded.rooms.contains_key("!sent:test.example.com"));
        assert_eq!(
            loaded.thread_revision("!room:test.example.com", "$root:test.example.com"),
            12.into()
        );
        assert!(
            loaded
                .threads
                .get("!room:test.example.com")
                .and_then(|threads| threads.get("$root:test.example.com"))
                .is_none()
        );

        let mut corrupted = bytes;
        let last = corrupted.last_mut().expect("non-empty encrypted payload");
        *last ^= 0x01;
        std::fs::write(&path, corrupted).expect("write corrupted drafts");
        assert!(matches!(
            actor.load_composer_drafts(&key_id),
            Err(CoreFailure::StoreUnavailable)
        ));
    }

    #[test]
    fn failed_composer_draft_atomic_replace_preserves_previous_payload_exactly() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);

        let mut old = ComposerDraftStore::default();
        old.apply_room_draft(
            "!room:test.example.com".to_owned(),
            "old synthetic draft".to_owned(),
            1.into(),
        )
        .expect("seed old draft");
        actor
            .save_composer_drafts(&key_id, &persisted_composer_drafts(&old))
            .expect("save old encrypted payload");
        let path = actor.account_composer_drafts_file(&key_id);
        let old_payload = std::fs::read(&path).expect("read old encrypted payload");

        let mut new = old.clone();
        new.apply_room_draft(
            "!room:test.example.com".to_owned(),
            "new synthetic draft".to_owned(),
            2.into(),
        )
        .expect("seed new draft");
        actor.fail_next_composer_draft_replace_for_testing();
        assert!(matches!(
            actor.save_composer_drafts(&key_id, &persisted_composer_drafts(&new)),
            Err(CoreFailure::StoreUnavailable)
        ));
        assert!(
            std::fs::read(&path).is_ok_and(|payload| payload == old_payload),
            "a failed replacement must leave the previous encrypted payload byte-exact"
        );
        assert_eq!(
            actor
                .load_composer_drafts(&key_id)
                .expect("old encrypted payload remains readable"),
            old
        );
    }

    #[test]
    fn two_accounts_with_same_targets_migrate_and_collect_independently() {
        fn legacy_payload(label: &str, revision_base: u64) -> Vec<u8> {
            let room_revisions = (0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT)
                .map(|index| (format!("shared-{index:03}"), revision_base + index as u64))
                .collect::<std::collections::BTreeMap<_, _>>();
            serde_json::to_vec(&serde_json::json!({
                "rooms": {"shared-content": format!("{label}-body")},
                "threads": {"shared-content": {"root-shared": format!("{label}-thread")}},
                "room_revisions": room_revisions,
                "thread_revisions": {
                    "shared-content": {"root-shared": revision_base}
                }
            }))
            .expect("serialize legacy fixture")
        }

        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_a = make_key_id();
        let mut key_b = make_key_id();
        key_b.user_id = "@bob:test.example.com".to_owned();
        key_b.device_id = "DEVICE2".to_owned();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let path_a = actor.account_composer_drafts_file(&key_a);
        let path_b = actor.account_composer_drafts_file(&key_b);
        assert_ne!(path_a, path_b);

        for (key_id, path, payload) in [
            (&key_a, &path_a, legacy_payload("account-a", 10)),
            (&key_b, &path_b, legacy_payload("account-b", 1_000)),
        ] {
            let secret = actor
                .load_or_create_unlock_secret(key_id)
                .expect("seed account-local unlock secret");
            let encrypted = encrypt_composer_drafts_fixture_payload(&secret, &payload)
                .expect("encrypt legacy fixture");
            std::fs::create_dir_all(path.parent().expect("composer draft parent"))
                .expect("create composer draft parent");
            std::fs::write(path, encrypted).expect("write legacy encrypted payload");
        }

        let mut account_a = actor
            .load_composer_drafts(&key_a)
            .expect("migrate account A");
        let account_b = actor
            .load_composer_drafts(&key_b)
            .expect("migrate account B");
        assert_eq!(
            account_a
                .composer_for_thread("shared-content", "root-shared")
                .draft,
            "account-a-thread"
        );
        assert_eq!(
            account_b
                .composer_for_thread("shared-content", "root-shared")
                .draft,
            "account-b-thread"
        );

        actor
            .save_composer_drafts(&key_a, &persisted_composer_drafts(&account_a))
            .expect("write account A v2");
        actor
            .save_composer_drafts(&key_b, &persisted_composer_drafts(&account_b))
            .expect("write account B v2");

        assert!(
            account_a
                .apply_room_draft("zz-new".to_owned(), String::new(), 1.into())
                .expect("collect account A independently")
        );
        actor
            .save_composer_drafts(&key_a, &persisted_composer_drafts(&account_a))
            .expect("write collected account A v2");

        let account_a = actor
            .load_composer_drafts(&key_a)
            .expect("reload collected account A");
        let account_b = actor
            .load_composer_drafts(&key_b)
            .expect("reload untouched account B");
        assert!(account_a.room_revision("shared-000").is_zero());
        assert_eq!(account_a.room_revision("zz-new"), 1.into());
        assert_eq!(account_b.room_revision("shared-000"), 1_000.into());
        assert!(account_b.room_revision("zz-new").is_zero());
        assert_eq!(
            account_b.composer_for_room("shared-content").draft,
            "account-b-body"
        );
    }

    #[test]
    fn legacy_composer_draft_payload_defaults_causal_revisions() {
        let legacy = r#"{
            "rooms":{"!room:test.example.com":"legacy room draft"},
            "threads":{"!room:test.example.com":{"$root:test.example.com":"legacy thread draft"}}
        }"#;

        let loaded: ComposerDraftStore =
            serde_json::from_str(legacy).expect("deserialize legacy draft payload");

        assert_eq!(loaded.room_revision("!room:test.example.com"), 0.into());
        assert_eq!(
            loaded.thread_revision("!room:test.example.com", "$root:test.example.com"),
            0.into()
        );
        assert_eq!(
            loaded.composer_for_room("!room:test.example.com").draft,
            "legacy room draft"
        );
        assert_eq!(
            loaded
                .composer_for_thread("!room:test.example.com", "$root:test.example.com")
                .draft,
            "legacy thread draft"
        );
    }

    #[test]
    fn loading_composer_drafts_does_not_create_missing_credentials() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let path = actor.account_composer_drafts_file(&key_id);
        std::fs::create_dir_all(path.parent().expect("draft parent")).expect("create parent");
        std::fs::write(&path, COMPOSER_DRAFTS_FILE_MAGIC).expect("write draft placeholder");

        assert!(matches!(
            actor.load_composer_drafts(&key_id),
            Err(CoreFailure::LocalEncryptionUnavailable)
        ));
        let missing = actor.credential_backend().load(&key_id).unwrap_err();
        assert!(koushi_key::is_missing_credential_error(&missing));
    }

    #[test]
    fn empty_composer_drafts_remove_persisted_file() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let mut drafts = ComposerDraftStore::default();
        drafts.set_room_draft("!room:test.example.com".to_owned(), "draft".to_owned());

        actor
            .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
            .expect("save non-empty drafts");
        let path = actor.account_composer_drafts_file(&key_id);
        assert!(path.exists());

        actor
            .save_composer_drafts(
                &key_id,
                &persisted_composer_drafts(&ComposerDraftStore::default()),
            )
            .expect("save empty drafts");
        assert!(!path.exists());
        assert!(
            actor
                .load_composer_drafts(&key_id)
                .expect("load removed drafts")
                .is_empty()
        );
    }

    #[test]
    fn composer_draft_persistence_keeps_content_and_applies_size_bounds() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let mut drafts = ComposerDraftStore::default();
        let oversized = "x".repeat(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES + 64);

        for index in 0..(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT + 8) {
            drafts.set_room_draft(format!("!room-{index}:test.example.com"), oversized.clone());
        }
        for index in 0..(koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT + 8) {
            drafts.set_thread_draft(
                "!thread-room:test.example.com".to_owned(),
                format!("$root-{index}"),
                oversized.clone(),
            );
        }

        actor
            .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
            .expect("save bounded drafts");
        let loaded = actor
            .load_composer_drafts(&key_id)
            .expect("load bounded drafts");

        assert_eq!(
            loaded.rooms.len(),
            koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT + 8
        );
        assert!(
            loaded
                .rooms
                .values()
                .all(|draft| draft.len() <= koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES)
        );
        let thread_count = loaded
            .threads
            .values()
            .map(std::collections::BTreeMap::len)
            .sum::<usize>();
        assert_eq!(
            thread_count,
            koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT + 8
        );
        assert!(
            loaded
                .threads
                .values()
                .flat_map(|room_threads| room_threads.values())
                .all(|draft| draft.len() <= koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_BYTES)
        );
    }

    #[test]
    fn composer_draft_persistence_prioritizes_content_over_revision_tombstones() {
        let data_dir = tempdir().expect("tempdir");
        let cred_dir = tempdir().expect("tempdir");
        let key_id = make_key_id();
        let actor = file_store_actor(&data_dir, &cred_dir);
        let mut drafts = ComposerDraftStore::default();

        for index in 0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_ROOM_COUNT {
            assert!(
                drafts
                    .apply_room_draft(
                        format!("!a-tombstone-{index:04}:test.example.com"),
                        String::new(),
                        1.into(),
                    )
                    .expect("room tombstone should apply")
            );
        }
        for index in 0..koushi_state::MAX_PERSISTED_COMPOSER_DRAFT_THREAD_COUNT {
            assert!(
                drafts
                    .apply_thread_draft(
                        "!thread-room:test.example.com".to_owned(),
                        format!("$a-tombstone-{index:04}"),
                        String::new(),
                        1.into(),
                    )
                    .expect("thread tombstone should apply")
            );
        }
        drafts.set_room_draft(
            "!z-active:test.example.com".to_owned(),
            "active room draft".to_owned(),
        );
        drafts.set_thread_draft(
            "!thread-room:test.example.com".to_owned(),
            "$z-active".to_owned(),
            "active thread draft".to_owned(),
        );

        actor
            .save_composer_drafts(&key_id, &persisted_composer_drafts(&drafts))
            .expect("save bounded drafts");
        let loaded = actor
            .load_composer_drafts(&key_id)
            .expect("load bounded drafts");

        assert_eq!(
            loaded
                .rooms
                .get("!z-active:test.example.com")
                .map(koushi_state::ComposerDocument::plain_body),
            Some("active room draft".to_owned())
        );
        assert_eq!(
            loaded
                .threads
                .get("!thread-room:test.example.com")
                .and_then(|threads| threads.get("$z-active"))
                .map(koushi_state::ComposerDocument::plain_body),
            Some("active thread draft".to_owned())
        );
    }
}
