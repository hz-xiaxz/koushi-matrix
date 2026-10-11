use super::*;
use crate::store::test_support::{file_store_actor, make_key_id};
use tempfile::tempdir;

fn sample_identity() -> SearchProgressIdentity {
    SearchProgressIdentity {
        index_contract_version: 7,
        index_instance: "instance-a".to_owned(),
        include_media_captions: true,
        include_filenames: false,
    }
}

fn sample_progress() -> SearchProgress {
    let mut progress = SearchProgress::new(sample_identity());
    let room = progress.room_mut("!room:example.invalid");
    room.backfill_complete = true;
    room.latest_event_id = Some("$latest:example.invalid".to_owned());
    room.processed = 41;
    room.indexed = 39;
    room.queue_pending_event("$late:example.invalid".to_owned());
    progress
}

#[test]
fn search_progress_round_trips_encrypted_and_rejects_corruption() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let progress = sample_progress();

    actor
        .save_search_progress(&key_id, &progress)
        .expect("save");

    let path = actor.account_search_progress_path(&key_id);
    let bytes = std::fs::read(&path).expect("read encrypted progress");
    assert!(
        !bytes.windows(12).any(|window| window == b"!room:exampl"),
        "room ids must not appear in the clear"
    );
    assert_eq!(
        actor.load_search_progress(&key_id).expect("load"),
        Some(progress)
    );

    let mut corrupted = bytes;
    let last = corrupted.last_mut().expect("non-empty payload");
    *last ^= 0x01;
    std::fs::write(&path, &corrupted).expect("write corrupted progress");
    assert_eq!(
        actor.load_search_progress(&key_id),
        Err(CoreFailure::StoreUnavailable),
        "a corrupted record must be reported, never trusted"
    );
}

#[test]
fn missing_progress_is_empty_and_an_empty_record_removes_the_file() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);

    assert_eq!(actor.load_search_progress(&key_id).expect("load"), None);

    actor
        .save_search_progress(&key_id, &SearchProgress::new(sample_identity()))
        .expect("save empty");
    assert!(!actor.account_search_progress_path(&key_id).exists());

    actor
        .save_search_progress(&key_id, &sample_progress())
        .expect("save");
    assert!(actor.account_search_progress_path(&key_id).exists());
    actor.delete_search_progress(&key_id).expect("delete");
    assert_eq!(actor.load_search_progress(&key_id).expect("load"), None);
}

#[test]
fn a_record_from_another_identity_is_not_trusted() {
    let progress = sample_progress();
    let matching = sample_identity();
    assert_eq!(progress.rooms_for(&matching).len(), 1);

    for changed in [
        SearchProgressIdentity {
            index_contract_version: 8,
            ..matching.clone()
        },
        SearchProgressIdentity {
            index_instance: "instance-b".to_owned(),
            ..matching.clone()
        },
        SearchProgressIdentity {
            include_media_captions: false,
            ..matching.clone()
        },
        SearchProgressIdentity {
            include_filenames: true,
            ..matching.clone()
        },
    ] {
        assert!(
            progress.rooms_for(&changed).is_empty(),
            "a changed identity must re-crawl instead of trusting the record: {changed:?}"
        );
    }

    let mut adopted = progress.clone();
    adopted.adopt_identity(changed_identity());
    assert!(
        adopted.is_empty(),
        "adopting a new identity drops the old rooms"
    );
}

fn changed_identity() -> SearchProgressIdentity {
    SearchProgressIdentity {
        index_instance: "instance-b".to_owned(),
        ..sample_identity()
    }
}

#[test]
fn pending_late_decryption_is_bounded_and_never_skipped() {
    let mut room = RoomCrawlProgress {
        backfill_complete: true,
        ..RoomCrawlProgress::default()
    };
    assert!(room.is_settled());

    room.queue_pending_event("$one:example.invalid".to_owned());
    assert!(room.has_pending_work());
    assert!(
        !room.is_settled(),
        "a queued late decryption must not be skipped"
    );

    for index in 0..SEARCH_PROGRESS_PENDING_EVENTS_MAX {
        room.queue_pending_event(format!("$event{index}:example.invalid"));
    }
    assert!(
        room.pending_overflow,
        "the list must fall back to a whole-room re-read"
    );
    assert!(
        room.pending_events.is_empty(),
        "the overflow form stores no ids"
    );
    assert!(!room.is_settled());
}

#[test]
fn the_index_instance_marker_is_created_once_and_survives_reads() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);

    let first = actor.search_index_instance(&key_id).expect("instance");
    assert!(!first.is_empty());
    assert_eq!(
        actor.search_index_instance(&key_id).expect("instance"),
        first
    );

    // Deleting the index directory (and so the marker) produces a new instance,
    // which is what invalidates a record written against the old index.
    let index_dir = actor.account_search_index_dir(&key_id);
    std::fs::remove_dir_all(&index_dir).expect("remove index directory");
    assert_ne!(
        actor.search_index_instance(&key_id).expect("instance"),
        first
    );
}

#[test]
fn an_identity_carries_the_contract_version_and_content_policy() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);

    let identity = actor
        .search_progress_identity(&key_id, 3, true, false)
        .expect("identity");
    assert_eq!(identity.index_contract_version, 3);
    assert!(identity.include_media_captions);
    assert!(!identity.include_filenames);
    assert_eq!(
        identity.index_instance,
        actor.search_index_instance(&key_id).expect("instance")
    );
}
