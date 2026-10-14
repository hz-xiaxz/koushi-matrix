//! Durable search-crawl progress.
//!
//! A restart used to forget which rooms had already been indexed, so every launch
//! began the same full-history crawl. This module persists one small encrypted
//! file per account holding, for each crawled room, the committed boundary, the
//! counters the room row reports, and whether a late decryption has since changed
//! the room in place.
//!
//! Only identifiers and counters are stored — room ids, event ids and counts,
//! never message text.
//!
//! An *unfinished* backfill is deliberately not recorded. Its position would be a
//! persisted chunk cursor, and resuming from one cannot be made sound: events
//! that arrived while the app was closed sit above that cursor behind a gap the
//! resumed walk never crosses, so the room could be marked complete with that gap
//! unindexed. An interrupted room is therefore crawled again from its newest
//! event, which is idempotent and bounded by one page per replay.

use super::{CoreFailure, StoreActor};
use koushi_protocol::SessionKeyId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const SEARCH_PROGRESS_FILE_MAGIC: &[u8] = b"KOUSHI-SEARCH-PROGRESS-V1\0";

/// Everything that decides whether a persisted record still describes the index
/// it was written against.
///
/// The record is only trusted when every field matches the running session. A
/// mismatch re-crawls instead of trusting a stale commitment, which is what makes
/// an extraction, content-policy or store change a migration rather than a silent
/// skip.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SearchProgressIdentity {
    /// The index contract version whose directory this record describes.
    pub index_contract_version: u32,
    /// The index instance this record describes.
    ///
    /// The value is a marker written inside the account's index directory, so
    /// deleting and recreating that directory invalidates the record: a fresh
    /// empty index would otherwise look like an index that already holds these
    /// rooms. The marker identifies the directory, not each room's subdirectory;
    /// the residual is recorded in the plan.
    pub index_instance: String,
    pub include_media_captions: bool,
    pub include_filenames: bool,
}

/// What is known about one room's crawl.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct RoomCrawlProgress {
    /// Whether the room's historical backfill reached the start of its known
    /// history and its documents are committed to the index.
    pub backfill_complete: bool,
    /// Latest event id recorded when the backfill completed; a later change to
    /// the room's newest event queues a catch-up bounded by this (#996).
    pub latest_event_id: Option<String>,
    pub processed: u64,
    pub indexed: u64,
    /// Whether a late decryption changed events of this room in place since the
    /// backfill committed.
    ///
    /// A resolution rewrites the stored event without adding anything newer, so
    /// no tail comparison can notice it: the room is re-read from its persisted
    /// chunks instead of being skipped. The flag is per room, so no per-event
    /// bookkeeping can grow with a session's decryption traffic.
    pub needs_reread: bool,
}

impl RoomCrawlProgress {
    /// Whether the room can be skipped on the next session.
    pub(crate) fn is_settled(&self) -> bool {
        self.backfill_complete && !self.needs_reread
    }
}

/// Committed crawl state for one account.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SearchProgress {
    identity: SearchProgressIdentity,
    rooms: BTreeMap<String, RoomCrawlProgress>,
}

impl SearchProgress {
    pub(crate) fn new(identity: SearchProgressIdentity) -> Self {
        Self {
            identity,
            rooms: BTreeMap::new(),
        }
    }

    /// The rooms this record describes, when it still describes the running
    /// index. A mismatch is a migration, so nothing is trusted.
    pub(crate) fn rooms_for(
        &self,
        identity: &SearchProgressIdentity,
    ) -> &BTreeMap<String, RoomCrawlProgress> {
        if &self.identity == identity {
            &self.rooms
        } else {
            &EMPTY_ROOMS
        }
    }

    pub(crate) fn room(&self, room_id: &str) -> Option<&RoomCrawlProgress> {
        self.rooms.get(room_id)
    }

    pub(crate) fn room_mut(&mut self, room_id: &str) -> &mut RoomCrawlProgress {
        self.rooms.entry(room_id.to_owned()).or_default()
    }

    pub(crate) fn forget(&mut self, room_id: &str) {
        self.rooms.remove(room_id);
    }

    /// Drop every room, keeping the identity: an explicit invalidation says the
    /// recorded completions no longer describe the index.
    pub(crate) fn clear_rooms(&mut self) {
        self.rooms.clear();
    }

    /// Adopt a new identity, dropping what was recorded under the old one.
    pub(crate) fn adopt_identity(&mut self, identity: SearchProgressIdentity) {
        if self.identity != identity {
            self.identity = identity;
            self.rooms.clear();
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }
}

static EMPTY_ROOMS: BTreeMap<String, RoomCrawlProgress> = BTreeMap::new();

/// Name of the marker file inside the account's index directory.
///
/// It lives inside the directory so that deleting the directory deletes it: a
/// recreated empty index then produces a new instance id and every record
/// written against the old one is not trusted.
const SEARCH_INDEX_INSTANCE_FILE: &str = ".koushi-index-instance";

/// A fresh index-directory instance id.
pub(crate) fn new_index_instance_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}-{}", std::process::id())
}

impl StoreActor {
    /// Read, or create once, the account's index-directory instance id.
    pub(crate) fn search_index_instance(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<String, CoreFailure> {
        let path = self
            .account_search_index_dir(key_id)
            .join(SEARCH_INDEX_INSTANCE_FILE);
        match std::fs::read_to_string(&path) {
            Ok(instance) => {
                let instance = instance.trim();
                if !instance.is_empty() {
                    return Ok(instance.to_owned());
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        }

        let instance = new_index_instance_id();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| CoreFailure::StoreUnavailable)?;
        }
        std::fs::write(&path, &instance).map_err(|_| CoreFailure::StoreUnavailable)?;
        Ok(instance)
    }

    /// Load the account's crawl progress.
    ///
    /// A missing file is an account that has never crawled, which is empty
    /// progress rather than a failure; anything unreadable or undecryptable is
    /// reported as `StoreUnavailable` so the caller can crawl again.
    pub(crate) fn load_search_progress(
        &self,
        key_id: &SessionKeyId,
    ) -> Result<Option<SearchProgress>, CoreFailure> {
        let path = self.account_search_progress_path(key_id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(CoreFailure::StoreUnavailable),
        };

        decrypt_search_progress_payload(&self.load_unlock_secret(key_id)?, &bytes).map(Some)
    }

    /// Write the account's crawl progress, replacing the previous record.
    ///
    /// An empty record removes the file, so an account with nothing committed
    /// leaves no stale commitment behind.
    pub(crate) fn save_search_progress(
        &self,
        key_id: &SessionKeyId,
        progress: &SearchProgress,
    ) -> Result<(), CoreFailure> {
        let path = self.account_search_progress_path(key_id);
        if progress.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(CoreFailure::StoreUnavailable),
            };
        }

        let payload =
            encrypt_search_progress_payload(&self.load_or_create_unlock_secret(key_id)?, progress)?;
        koushi_store::atomic_replace_file(&path, &payload, false)
            .map_err(|_| CoreFailure::StoreUnavailable)
    }

    /// Path of the account's crawl-progress file.
    pub(crate) fn account_search_progress_path(&self, key_id: &SessionKeyId) -> PathBuf {
        self.account_root_dir(key_id)
            .join("search")
            .join("progress.v1.enc")
    }
}

fn encrypt_search_progress_payload(
    secret: &koushi_key::LocalUnlockSecret,
    progress: &SearchProgress,
) -> Result<Vec<u8>, CoreFailure> {
    let plaintext = serde_json::to_vec(progress).map_err(|_| CoreFailure::StoreUnavailable)?;
    let key = secret.derive_search_progress_key();
    koushi_store::encrypt_envelope(
        SEARCH_PROGRESS_FILE_MAGIC,
        key.as_bytes(),
        &plaintext,
        usize::MAX,
    )
    .map_err(|_| CoreFailure::StoreUnavailable)
}

fn decrypt_search_progress_payload(
    secret: &koushi_key::LocalUnlockSecret,
    payload: &[u8],
) -> Result<SearchProgress, CoreFailure> {
    let key = secret.derive_search_progress_key();
    let plaintext = koushi_store::decrypt_envelope(
        SEARCH_PROGRESS_FILE_MAGIC,
        key.as_bytes(),
        payload,
        usize::MAX,
    )
    .map_err(|_| CoreFailure::StoreUnavailable)?;
    serde_json::from_slice(&plaintext).map_err(|_| CoreFailure::StoreUnavailable)
}

#[cfg(test)]
mod tests;
