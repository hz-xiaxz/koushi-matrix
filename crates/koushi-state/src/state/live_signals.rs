use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::profile::{
    AvatarImage, ProfileResolutionInput, ProfileState, UserProfile,
    resolve_optional_user_display_name, resolve_people_label,
};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveSignalsState {
    pub rooms: BTreeMap<String, RoomLiveSignals>,
    pub presence: BTreeMap<String, PresenceKind>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoomLiveSignals {
    pub receipts_by_event: BTreeMap<String, LiveEventReceiptSummary>,
    pub fully_read_event_id: Option<String>,
    pub typing_user_ids: Vec<String>,
    #[serde(default)]
    pub typing_users: Vec<LiveTypingUser>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveTypingUser {
    pub user_id: String,
    pub display_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveReadReceipt {
    pub user_id: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub original_display_label: String,
    pub avatar: Option<AvatarImage>,
    pub timestamp_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveEventReceiptSummary {
    pub readers: Vec<LiveReadReceipt>,
    pub total_count: u64,
    pub overflow_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveEventReceipts {
    pub event_id: String,
    pub receipts: Vec<LiveReadReceipt>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveRoomSignalUpdate {
    pub receipts_by_event: Vec<LiveEventReceipts>,
    pub fully_read_event_id: Option<String>,
    pub typing_user_ids: Vec<String>,
}

impl LiveRoomSignalUpdate {
    pub fn into_room_signals(self) -> RoomLiveSignals {
        self.into_room_signals_with_profiles(&ProfileState::default(), None)
    }

    pub fn into_room_signals_with_profiles(
        self,
        profiles: &ProfileState,
        own_user_id: Option<&str>,
    ) -> RoomLiveSignals {
        self.into_room_signals_with_room_profiles(profiles, None, own_user_id)
    }

    pub fn into_room_signals_with_room_profiles(
        self,
        profiles: &ProfileState,
        relevant_room_profiles: Option<&BTreeMap<String, UserProfile>>,
        own_user_id: Option<&str>,
    ) -> RoomLiveSignals {
        let receipts_by_event = self
            .receipts_by_event
            .into_iter()
            .map(|entry| {
                let receipts = normalize_receipts(
                    entry.receipts,
                    profiles,
                    relevant_room_profiles,
                    own_user_id,
                );
                (entry.event_id, receipts)
            })
            .collect();

        let typing_user_ids = sorted_unique(self.typing_user_ids);
        let typing_users = typing_user_ids
            .iter()
            .map(|user_id| LiveTypingUser {
                user_id: user_id.clone(),
                display_label: resolve_optional_user_display_name(
                    profiles,
                    user_id,
                    None,
                    own_user_id,
                ),
            })
            .collect();
        RoomLiveSignals {
            receipts_by_event,
            fully_read_event_id: self.fully_read_event_id,
            typing_user_ids,
            typing_users,
        }
    }
}

pub fn refresh_live_typing_user_display_projection(
    live_signals: &mut LiveSignalsState,
    profiles: &ProfileState,
    own_user_id: Option<&str>,
) -> bool {
    let mut changed = false;
    for room in live_signals.rooms.values_mut() {
        for typing_user in &mut room.typing_users {
            let display_label = resolve_optional_user_display_name(
                profiles,
                &typing_user.user_id,
                None,
                own_user_id,
            );
            if typing_user.display_label != display_label {
                typing_user.display_label = display_label;
                changed = true;
            }
        }
    }
    changed || refresh_live_receipt_display_projection(live_signals, profiles, own_user_id)
}

pub fn refresh_live_receipt_display_projection(
    live_signals: &mut LiveSignalsState,
    profiles: &ProfileState,
    own_user_id: Option<&str>,
) -> bool {
    let mut changed = false;
    for (room_id, room) in live_signals.rooms.iter_mut() {
        let relevant_room_profiles = profiles.room_users.get(room_id);
        for summary in room.receipts_by_event.values_mut() {
            for receipt in &mut summary.readers {
                let enriched = enrich_receipt(
                    receipt.clone(),
                    profiles,
                    relevant_room_profiles,
                    own_user_id,
                );
                if receipt.display_name != enriched.display_name
                    || receipt.original_display_label != enriched.original_display_label
                    || receipt.avatar != enriched.avatar
                {
                    *receipt = enriched;
                    changed = true;
                }
            }
        }
    }
    changed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresenceKind {
    Online,
    Away,
    Offline,
}

fn normalize_receipts(
    receipts: Vec<LiveReadReceipt>,
    profiles: &ProfileState,
    relevant_room_profiles: Option<&BTreeMap<String, UserProfile>>,
    own_user_id: Option<&str>,
) -> LiveEventReceiptSummary {
    let mut by_user = BTreeMap::new();
    for receipt in receipts {
        // Exclude the current user's own receipts before building the readers
        // list — own reads (including reads on other devices) must not appear
        // in the displayed readers or affect the counts.
        if own_user_id.is_some_and(|own| own == receipt.user_id) {
            continue;
        }
        let receipt = enrich_receipt(receipt, profiles, relevant_room_profiles, own_user_id);
        by_user
            .entry(receipt.user_id.clone())
            .and_modify(|existing: &mut LiveReadReceipt| {
                if receipt_is_newer(&receipt, existing) {
                    *existing = receipt.clone();
                }
            })
            .or_insert(receipt);
    }
    let mut readers = by_user.into_values().collect::<Vec<_>>();
    readers.sort_by(|left, right| {
        right
            .timestamp_ms
            .unwrap_or_default()
            .cmp(&left.timestamp_ms.unwrap_or_default())
            .then_with(|| left.user_id.cmp(&right.user_id))
    });

    let total_count = readers.len() as u64;

    LiveEventReceiptSummary {
        readers,
        total_count,
        overflow_count: 0,
    }
}

fn receipt_is_newer(candidate: &LiveReadReceipt, existing: &LiveReadReceipt) -> bool {
    candidate.timestamp_ms.unwrap_or_default() >= existing.timestamp_ms.unwrap_or_default()
}

fn enrich_receipt(
    mut receipt: LiveReadReceipt,
    profiles: &ProfileState,
    relevant_room_profiles: Option<&BTreeMap<String, UserProfile>>,
    own_user_id: Option<&str>,
) -> LiveReadReceipt {
    let own_profile = own_user_id
        .filter(|user_id| *user_id == receipt.user_id)
        .map(|_| &profiles.own);
    let relevant_room_profile =
        relevant_room_profiles.and_then(|room_profiles| room_profiles.get(&receipt.user_id));
    let user_profile = profiles.users.get(&receipt.user_id);

    let receipt_display_name = receipt
        .display_name
        .clone()
        .filter(|label| label.trim() != "Unknown user");
    let receipt_original_display_label =
        (!receipt.original_display_label.trim().eq("Unknown user"))
            .then(|| receipt.original_display_label.clone());
    let original_source = receipt_original_display_label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
        .or(receipt_display_name.as_deref())
        .or_else(|| relevant_room_profile.and_then(|profile| profile.display_name.as_deref()))
        .or_else(|| user_profile.and_then(|profile| profile.display_name.as_deref()))
        .or_else(|| own_profile.and_then(|profile| profile.display_name.as_deref()));
    let display_label = resolve_people_label(ProfileResolutionInput {
        local_alias: profiles
            .local_aliases
            .get(&receipt.user_id)
            .map(String::as_str),
        relevant_room_label: relevant_room_profile
            .and_then(|profile| profile.display_name.as_deref()),
        space_room_label: None,
        payload_label: receipt_display_name.as_deref(),
        cached_label: user_profile.and_then(|profile| profile.display_name.as_deref()),
        local_homeserver_label: own_profile.and_then(|profile| profile.display_name.as_deref()),
    })
    .label;
    let original_display_label = original_source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            user_profile
                .and_then(|profile| profile.display_name.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            own_profile
                .and_then(|profile| profile.display_name.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Unknown user".to_owned());
    receipt.display_name = Some(display_label);
    receipt.original_display_label = original_display_label;
    if receipt.avatar.is_none() {
        receipt.avatar = relevant_room_profile
            .and_then(|profile| profile.avatar.clone())
            .or_else(|| own_profile.and_then(|profile| profile.avatar.clone()))
            .or_else(|| user_profile.and_then(|profile| profile.avatar.clone()));
    }
    receipt
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}
