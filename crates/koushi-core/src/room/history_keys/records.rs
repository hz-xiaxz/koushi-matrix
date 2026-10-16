//! Private encrypted-store records. Never derive Debug or expose these in DTOs.
use super::policy::{REQUEST_LIFETIME, Receipt};
use koushi_state::HistoryKeyRecoveryFailure as Failure;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(super) struct Pin {
    pub user: String,
    pub device: String,
    pub curve: String,
    pub ed: String,
    pub master: String,
}

impl Pin {
    pub fn key(&self) -> String {
        format!("{}\n{}", self.user, self.device)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Attempt {
    pub pin: Pin,
    pub nonce: String,
    pub expires: u64,
    pub confirmed: bool,
    pub terminal: bool,
    pub receipt: Receipt,
}

impl Attempt {
    pub fn matches(&self, pin: &Pin, nonce: &str, now: u64) -> bool {
        self.pin == *pin && self.nonce == nonce && self.expires > now
    }
    pub fn complete(&self) -> bool {
        self.receipt.count > 0 && self.receipt.completed.len() == usize::from(self.receipt.count)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Pending {
    pub expires: u64,
    pub pins: Vec<Pin>,
    pub dispatch: Vec<Pin>,
    pub attempts: BTreeMap<String, Attempt>,
    pub force_all: bool,
    pub automatic: bool,
    pub generation: u64,
    pub auto_at: u64,
    pub sent_at: u64,
    pub sent: bool,
    pub received: u32,
    pub partial_reason: Option<Failure>,
    pub bytes_by_user: BTreeMap<String, usize>,
    pub bytes: usize,
    pub unresolved: bool,
}

impl Pending {
    pub fn recipients(&self) -> u16 {
        self.dispatch
            .iter()
            .map(|p| &p.user)
            .collect::<std::collections::BTreeSet<_>>()
            .len() as u16
    }
    pub fn responses(&self) -> u16 {
        self.dispatch
            .iter()
            .filter(|p| {
                self.attempts
                    .get(&p.key())
                    .is_some_and(|a| a.pin == **p && a.complete())
            })
            .map(|p| &p.user)
            .collect::<std::collections::BTreeSet<_>>()
            .len() as u16
    }
    pub fn chunk_count(&self) -> usize {
        self.attempts.values().map(|a| a.receipt.chunks.len()).sum()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Completed {
    pub room: String,
    pub pin: Pin,
    pub nonce: String,
    pub response: String,
    pub expires: u64,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub(super) enum CacheState {
    Preparing,
    Ready,
    Deleting,
    Tombstone,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Cached {
    pub room: String,
    pub pin: Pin,
    pub nonce: String,
    pub expires: u64,
    pub state: CacheState,
    pub bytes: usize,
    pub response: String,
}

impl Cached {
    pub fn storage_key(&self) -> String {
        format!(
            "chat.koushi.history_keys.reply.{:x}",
            Sha256::digest(nonce_key(&self.pin, &self.nonce))
        )
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Ledger {
    pub generation: u64,
    pub rooms: BTreeMap<String, Pending>,
    pub completed: BTreeMap<String, Completed>,
    pub cached: BTreeMap<String, Cached>,
    pub responded: BTreeMap<String, u64>,
    pub egress_started: u64,
    pub egress_bytes: usize,
}

pub(super) fn episode_expiry(now: u64) -> u64 {
    now.saturating_add(REQUEST_LIFETIME)
}

pub(super) fn nonce_key(pin: &Pin, nonce: &str) -> String {
    format!("{}\n{}", pin.key(), nonce)
}

impl Ledger {
    pub fn correlated(&self, room: &str, nonce: &str, user: &str, now: u64) -> bool {
        self.rooms.get(room).is_some_and(|p| {
            p.attempts.values().any(|a| {
                a.nonce == nonce && a.expires > now && a.pin.user == user && p.pins.contains(&a.pin)
            })
        }) || self
            .completed
            .values()
            .any(|c| c.room == room && c.nonce == nonce && c.pin.user == user && c.expires > now)
    }
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
