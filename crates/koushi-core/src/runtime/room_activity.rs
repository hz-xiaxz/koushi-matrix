//! #1364: persisted per-room conversation activity.
//!
//! The cache is loaded once per session, before the first room-list projection
//! can install rooms, and written only when the reducer's map advances. Writes
//! are immediate rather than debounced: the map moves forward when a room's
//! newest conversation activity changes, which is at most once per observed
//! message, and a room-list update that changes no activity writes nothing.

use super::{AppActor, room_preferences_session_key};
use crate::executor;
use koushi_state::{AppAction, RoomActivityState, reduce};

impl AppActor {
    pub(super) async fn load_room_activity_for_current_session(&mut self) {
        let Some(key_id) = room_preferences_session_key(&self.state) else {
            self.room_activity_loaded_for = None;
            return;
        };
        if self.room_activity_loaded_for.as_ref() == Some(&key_id) {
            return;
        }

        let store = self.composer_draft_store_actor.clone();
        let load_key_id = key_id.clone();
        let activity = executor::spawn_blocking(move || {
            store.load_room_activity(&load_key_id).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let effects = reduce(&mut self.state, AppAction::RoomActivityLoaded { activity });
        self.room_activity_loaded_for = Some(key_id);
        self.handle_ui_event_effects(&effects).await;
    }

    pub(super) async fn persist_room_activity(
        &self,
        key_id: koushi_protocol::SessionKeyId,
        activity: RoomActivityState,
    ) -> bool {
        let store = self.composer_draft_store_actor.clone();
        let result =
            executor::spawn_blocking(move || store.save_room_activity(&key_id, &activity)).await;
        matches!(result, Ok(Ok(())))
    }
}
