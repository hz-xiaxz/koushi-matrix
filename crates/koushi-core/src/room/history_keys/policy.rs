use std::collections::{BTreeMap, BTreeSet};

use koushi_state::HistoryKeyRecoveryFailure as Failure;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const MESSAGE_LIMIT: usize = 32 * 1024;
pub(super) const USER_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const REQUEST_LIMIT: usize = 32 * 1024 * 1024;
pub(super) const MAX_CHUNKS: u16 = 256;
pub(super) const REQUEST_LIFETIME: u64 = 30 * 24 * 60 * 60;
pub(super) const AUTO_COOLDOWN: u64 = 24 * 60 * 60;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Receipt {
    pub id: String,
    pub count: u16,
    pub chunks: BTreeMap<u16, String>,
    pub completed: BTreeSet<u16>,
}

/// Validate and reserve before importing. The reservation is persisted before
/// keys are imported, so replay after a crash cannot increase the byte budget.
pub(super) fn reserve_chunk(
    receipt: &mut Receipt,
    id: &str,
    count: u16,
    index: u16,
    data: &[u8],
) -> Result<bool, Failure> {
    if data.len() > MESSAGE_LIMIT
        || count == 0
        || count > MAX_CHUNKS
        || index >= count
        || id.len() != 48
    {
        return Err(Failure::TooLarge);
    }
    if !receipt.id.is_empty() && (receipt.id != id || receipt.count != count) {
        return Err(Failure::InvalidResponse);
    }
    let digest = format!("{:x}", Sha256::digest(data));
    if let Some(previous) = receipt.chunks.get(&index) {
        return if previous == &digest {
            Ok(!receipt.completed.contains(&index))
        } else {
            Err(Failure::InvalidResponse)
        };
    }
    receipt.id = id.to_owned();
    receipt.count = count;
    receipt.chunks.insert(index, digest);
    Ok(true)
}

pub(super) fn targets(
    joined: &BTreeSet<String>,
    admins: &BTreeSet<String>,
    own: &str,
    force_all: bool,
    automatic: bool,
) -> Result<Vec<String>, Failure> {
    if !joined.contains(own) {
        return Err(Failure::MembershipChanged);
    }
    let all =
        koushi_state::history_key_request_targets_all(Some(joined.len()), force_all, automatic);
    let users: Vec<_> = joined
        .iter()
        .filter(|user| user.as_str() != own && (all || admins.contains(*user)))
        .cloned()
        .collect();
    if users.len() > 128 {
        return Err(Failure::TooLarge);
    }
    if users.is_empty() {
        return Err(Failure::NoRecipients);
    }
    Ok(users)
}

pub(super) fn automatic_due(last: u64, now: u64) -> bool {
    now.checked_sub(last)
        .is_some_and(|elapsed| elapsed >= AUTO_COOLDOWN)
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
