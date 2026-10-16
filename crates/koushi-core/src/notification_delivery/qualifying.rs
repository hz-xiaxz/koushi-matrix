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

use koushi_state::{NotificationSinkSettings, RoomAttentionCandidate, RoomAttentionKind};

use super::dwell::QualifyingRoom;

/// The rooms one destination may deliver right now, in the attention projection's own
/// ranking (#1460: every destination is its own lane with its own targets and timing).
///
/// The rules, in the order they apply:
///
/// - Nothing qualifies for a destination with no target set. There is no on/off switch;
///   an empty target list is how a destination sends nothing.
/// - Nothing qualifies while the window is focused, under the destination's
///   *only when inactive* rule. A focused device cancels the dwell and re-arms it on the
///   next transition, which is why the caller passes focus here rather than holding the
///   rooms back at send time.
/// - A watched room, or a room that is a child of a watched space, always qualifies: the
///   watch list is the user naming a room to be told about whatever the categories say.
///   The parent spaces are the room list's own answer for the current observation, so a
///   room added to a watched space later is covered without editing the list. A watch
///   never overrides read, mute, or focus: those rooms are not candidates at all.
/// - Otherwise the kind has to be on: a mention (which is also what a keyword hit looks
///   like, because the SDK folds `kind: content` rules into the highlight counter) under
///   `mentions` or `keywords`, a direct message under `direct_messages`. A plain group
///   message has no trigger of its own and is delivered only through the watch list.
///
/// [`QualifyingRoom::message_text`] is filled from the candidate's
/// [`RoomAttentionCandidate::newest_message_text`] only when the destination's effective
/// content mode carries text - its chosen mode under a consent that still matches its
/// kind, endpoint, and recipient. A names-only destination never holds the text at all.
pub(crate) fn qualifying_rooms(
    sink: &NotificationSinkSettings,
    candidates: &[RoomAttentionCandidate],
    focused: bool,
) -> Vec<QualifyingRoom> {
    if !sink.targets.has_any() {
        return Vec::new();
    }
    if sink.timing.only_when_unfocused && focused {
        return Vec::new();
    }
    let carries_text = sink.effective_content().carries_text();

    let mut qualifying = Vec::new();
    for candidate in candidates {
        if !qualifies_by_trigger(sink, candidate) {
            continue;
        }
        qualifying.push(QualifyingRoom {
            room_id: candidate.room_id.clone(),
            display_label: candidate.display_label.clone(),
            space_labels: candidate.parent_space_labels.clone(),
            kind: candidate.kind,
            direct_partner: candidate.direct_partner.clone(),
            unread: candidate.unread,
            mentions: candidate.mentions,
            message_text: if carries_text {
                candidate.newest_message_text.clone()
            } else {
                None
            },
        });
    }
    qualifying
}

/// Whether the user asked this destination to be told about this room, by trigger kind
/// or by name.
fn qualifies_by_trigger(
    sink: &NotificationSinkSettings,
    candidate: &RoomAttentionCandidate,
) -> bool {
    let targets = &sink.targets;
    let watched = targets.watches_room(&candidate.room_id)
        || candidate
            .parent_space_ids
            .iter()
            .any(|space_id| targets.watches_space(space_id));
    if watched {
        return true;
    }
    match candidate.kind {
        // A keyword hit arrives as a highlight, so the projection cannot separate it from a
        // mention; either flag on is enough for a mention-kind room.
        RoomAttentionKind::Mention => targets.mentions || targets.keywords,
        RoomAttentionKind::Dm => targets.direct_messages,
        RoomAttentionKind::Message => false,
    }
}

#[cfg(test)]
#[path = "qualifying/tests.rs"]
mod tests;
