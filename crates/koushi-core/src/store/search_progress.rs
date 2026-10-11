//! Durable search-crawl progress.
//!
//! A restart used to forget which rooms had already been indexed, so every launch
//! began the same full-history crawl. This module persists one small encrypted
//! file per account holding, for each crawled room, the committed boundary, the
//! counters the room row reports, the traversal state of an unfinished backfill,
//! and the late-decryption work that still has to be indexed.
//!
//! Only identifiers, cursors and counters are stored — room ids, event ids, chunk
//! cursors and counts, never message text.

use super::{CoreFailure, StoreActor};
use koushi_protocol::SessionKeyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const SEARCH_PROGRESS_FILE_MAGIC: &[u8] = b"KOUSHI-SEARCH-PROGRESS-V1\0";

/// How many late-decrypted event ids one room may queue durably before the room
/// falls back to a whole-room re-read.
///
/// The ids are identifiers, but an unbounded set would grow with a long session's
/// late-decryption traffic, and a room that resolved more events than this is
/// cheaper to re-read from its persisted chunks than to track one by one.
pub(crate) const SEARCH_PROGRESS_PENDING_EVENTS_MAX: usize = 512;

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
    /// rooms.
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
    /// Traversal state of an unfinished backfill: the committed cursor of the
    /// next persisted page to read.
    ///
    /// A cursor whose chunk no longer exists is not silently treated as the end
    /// of the store; the reader reports it, and the caller re-crawls the room from
    /// the tail.
    pub cursor: Option<u64>,
    /// Whether the persisted walk had already crossed a gap chunk, so completion
    /// cannot be claimed from the local store alone.
    pub crossed_gap: bool,
    /// Whether the traversal had switched to SDK back-pagination.
    pub network_source: bool,
    /// Late-decrypted events that still have to be indexed, because they changed
    /// in place and no later event of the room records that change.
    pub pending_events: BTreeSet<String>,
    /// Whether `pending_events` overflowed, so the whole room must be re-read
    /// rather than the listed events.
    pub pending_overflow: bool,
}

impl RoomCrawlProgress {
    /// Whether anything here still needs index work.
    pub(crate) fn has_pending_work(&self) -> bool {
        self.pending_overflow || !self.pending_events.is_empty()
    }

    /// Whether the room can be skipped on the next session.
    pub(crate) fn is_settled(&self) -> bool {
        self.backfill_complete && !self.has_pending_work()
    }

    /// Queue one late-decrypted event, falling back to a whole-room re-read once
    /// the bounded list is full.
    pub(crate) fn queue_pending_event(&mut self, event_id: String) {
        if self.pending_overflow || self.pending_events.len() >= SEARCH_PROGRESS_PENDING_EVENTS_MAX
        {
            self.pending_overflow = true;
            self.pending_events.clear();
            return;
        }
        self.pending_events.insert(event_id);
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

    pub(crate) fn identity(&self) -> &SearchProgressIdentity {
        &self.identity
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

impl StoreActor {
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

    /// Delete the account's crawl progress, e.g. because the index it described
    /// is gone.
    pub(crate) fn delete_search_progress(&self, key_id: &SessionKeyId) -> Result<(), CoreFailure> {
        match std::fs::remove_file(self.account_search_progress_path(key_id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(CoreFailure::StoreUnavailable),
        }
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
