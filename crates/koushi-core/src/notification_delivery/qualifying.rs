//! Which rooms this device may deliver, built from the attention projection itself.
//!
//! The qualifying set is the caller's half of the decision engine: the engine's
//! [`super::dwell`] holds whatever it is handed and [`super::digest`] renders it without
//! second-guessing it, so the enabled trigger kinds, the watch list, and the focus
//! suppression live here, in one function that reads one projection
//! (`docs/architecture/state-machine.md`, "Local Notification Delivery").
//!
//! The rooms come from [`koushi_state::attention_room_candidates`], which derives them
//! from the same read/unread and mute authority that builds the badge, so this module
//! never re-derives "is this room unread". It only decides which of those rooms the user
//! asked to be told about.

use koushi_state::{NotificationDeliverySettings, RoomAttentionCandidate, RoomAttentionKind};

use super::dwell::QualifyingRoom;

/// The rooms the delivery channel may deliver right now, in the attention projection's
/// own ranking.
///
/// The rules, in the order they apply:
///
/// - Nothing qualifies while the channel is off. [`NotificationDeliverySettings::enabled`]
///   is the channel's own switch; the three trigger flags below are meaningless without it.
/// - Nothing qualifies while the window is focused, under
///   [`NotificationDeliverySettings::only_when_unfocused`]. A focused device cancels the
///   dwell and re-arms it on the next transition, which is why the caller passes focus here
///   rather than holding the rooms back at send time.
/// - A watched room, or a room that is a child of a watched space, always qualifies: the
///   watch list is the user naming a room to be told about whatever the categories say. The
///   parent spaces are the room list's own answer for the current observation, so a room
///   added to a watched space later is covered without editing the list.
/// - Otherwise the kind has to be on: a mention (which is also what a keyword hit looks
///   like, because the SDK folds `kind: content` rules into the highlight counter) under
///   `mentions` or `keywords`, a direct message under `direct_messages`. A plain group
///   message has no trigger of its own and is delivered only through the watch list.
///
/// [`QualifyingRoom::plaintext_prefix`] is filled from the candidate's
/// [`RoomAttentionCandidate::newest_message_text`] whenever
/// [`NotificationDeliverySettings::prefix_chars`] is on, whatever any consent says: the
/// consent is per sink and per destination, so it is applied in [`super::digest`]. The text
/// is the body of the room's latest conversation event, which is the newest message this
/// device already holds and may be newer than the unread event that qualified the room.
pub(crate) fn qualifying_rooms(
    settings: &NotificationDeliverySettings,
    candidates: &[RoomAttentionCandidate],
    focused: bool,
) -> Vec<QualifyingRoom> {
    if !settings.enabled {
        return Vec::new();
    }
    if settings.only_when_unfocused && focused {
        return Vec::new();
    }

    let mut qualifying = Vec::new();
    for candidate in candidates {
        if !qualifies_by_trigger(settings, candidate) {
            continue;
        }
        qualifying.push(QualifyingRoom {
            room_id: candidate.room_id.clone(),
            display_label: candidate.display_label.clone(),
            kind: candidate.kind,
            unread: candidate.unread,
            mentions: candidate.mentions,
            // The character count is the whole decision here. Reading the text is already
            // done - the candidate carries what the room list holds - and whether a sink
            // may render it is a per-sink consent that `digest` applies.
            plaintext_prefix: if settings.prefix_chars > 0 {
                candidate.newest_message_text.clone()
            } else {
                None
            },
        });
    }
    qualifying
}

/// Whether the user asked to be told about this room, by trigger kind or by name.
fn qualifies_by_trigger(
    settings: &NotificationDeliverySettings,
    candidate: &RoomAttentionCandidate,
) -> bool {
    let watched = settings.watches_room(&candidate.room_id)
        || candidate
            .parent_space_ids
            .iter()
            .any(|space_id| settings.watches_space(space_id));
    if watched {
        return true;
    }
    match candidate.kind {
        // A keyword hit arrives as a highlight, so the projection cannot separate it from a
        // mention; either flag on is enough for a mention-kind room.
        RoomAttentionKind::Mention => settings.mentions || settings.keywords,
        RoomAttentionKind::Dm => settings.direct_messages,
        RoomAttentionKind::Message => false,
    }
}

#[cfg(test)]
#[path = "qualifying/tests.rs"]
mod tests;
