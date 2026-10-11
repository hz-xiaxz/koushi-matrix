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
    progress
}

#[test]
fn search_progress_round_trips_encrypted_and_rejects_corruption() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);
    let progress = sample_progress();

    actor.save_search_progress(&key_id, &progress).expect("save");

    let path = actor.account_search_progress_path(&key_id);
    let bytes = std::fs::read(&path).expect("read encrypted progress");
    assert!(
        !bytes.windows(12).any(|window| window == b"!room:exampl"),
        "room ids must not appear in the clear"
    );
    assert_eq!(actor.load_search_progress(&key_id).expect("load"), Some(progress));

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

    actor.save_search_progress(&key_id, &sample_progress()).expect("save");
    assert!(actor.account_search_progress_path(&key_id).exists());

    // An empty record removes the file rather than keeping an empty one.
    actor
        .save_search_progress(&key_id, &SearchProgress::new(sample_identity()))
        .expect("save empty");
    assert_eq!(actor.load_search_progress(&key_id).expect("load"), None);
}

#[test]
fn a_record_from_another_identity_is_not_trusted() {
    let progress = sample_progress();
    let matching = sample_progress();
    assert_eq!(
        matching.rooms_for(&sample_identity()).len(),
        1,
        "the matching identity is trusted"
    );
    assert_eq!(
        matching.rooms_for(&SearchProgressIdentity {
            index_contract_version: 8,
            ..sample_identity()
        }).len(),
        0,
        "another contract version is not trusted"
    );

    for changed in [
        SearchProgressIdentity { index_contract_version: 8, ..sample_identity() },
        SearchProgressIdentity { index_instance: "instance-b".to_owned(), ..sample_identity() },
        SearchProgressIdentity { include_media_captions: false, ..sample_identity() },
        SearchProgressIdentity { include_filenames: true, ..sample_identity() },
    ] {
        assert!(
            progress.rooms_for(&changed).is_empty(),
            "a changed identity must re-crawl instead of trusting the record: {changed:?}"
        );
        // Adoption of a changed identity also drops what it described.
        let mut adopted = progress.clone();
        adopted.adopt_identity(changed.clone());
        assert!(
            adopted.rooms_for(&changed).is_empty(),
            "an adopted identity never keeps the old rooms"
        );
    }

    // An explicit invalidation drops the rooms while keeping the identity.
    let mut invalidated = progress.clone();
    invalidated.clear_rooms();
    assert!(invalidated.is_empty());
    assert!(invalidated.rooms_for(&sample_identity()).is_empty());

    // Adopting the identity already held is not a way to clear the rooms.
    let mut unchanged = progress.clone();
    unchanged.adopt_identity(sample_identity());
    assert_eq!(unchanged.rooms_for(&sample_identity()).len(), 1);
}

#[test]
fn a_late_decryption_keeps_the_room_unsettled() {
    let mut room = RoomCrawlProgress { backfill_complete: true, ..RoomCrawlProgress::default() };
    assert!(room.is_settled(), "a committed backfill with no late decryption is settled");

    room.needs_reread = true;
    assert!(!room.is_settled(), "a late decryption must not be skipped");

    room.needs_reread = false;
    assert!(room.is_settled(), "a completed re-read settles the room again");
}

#[test]
fn the_index_instance_marker_is_created_once_and_survives_reads() {
    let data_dir = tempdir().expect("tempdir");
    let cred_dir = tempdir().expect("tempdir");
    let key_id = make_key_id();
    let actor = file_store_actor(&data_dir, &cred_dir);

    let first = actor.search_index_instance(&key_id).expect("instance");
    assert!(!first.is_empty());
    assert_eq!(actor.search_index_instance(&key_id).expect("instance"), first);

    // Deleting the index directory (and so the marker) produces a new instance,
    // which is what invalidates a record written against the old index.
    let index_dir = actor.account_search_index_dir(&key_id);
    std::fs::remove_dir_all(&index_dir).expect("remove index directory");
    assert_ne!(actor.search_index_instance(&key_id).expect("instance"), first);
}
