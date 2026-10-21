//! Scoped acknowledgement-to-canonical-SDK-commit ownership for topic writes.
use matrix_sdk::{
    Room,
    deserialized_responses::RawSyncOrStrippedState,
    ruma::{EventId, OwnedEventId, events::room::topic::RoomTopicEventContent},
    sync::{RoomUpdate, State},
};
use tokio::sync::broadcast;

use super::{MatrixRoomOperationError, MatrixRoomOperationFailureKind, MatrixRoomSettingsSnapshot};

const MAX_EARLY_TOPIC_EVENTS: usize = 32;
const TOPIC_WRITE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

fn unconfirmed() -> MatrixRoomOperationError {
    MatrixRoomOperationError::Sdk(MatrixRoomOperationFailureKind::Sdk)
}

struct TopicCommitBarrier {
    updates: broadcast::Receiver<RoomUpdate>,
    proofs: Vec<OwnedEventId>,
}
impl TopicCommitBarrier {
    fn new(room: &Room) -> Self {
        Self {
            updates: room.subscribe_to_updates(),
            proofs: Vec::new(),
        }
    }
    fn remember(&mut self, event: OwnedEventId) -> Result<(), MatrixRoomOperationError> {
        if self.proofs.contains(&event) {
            return Ok(());
        }
        if self.proofs.len() == MAX_EARLY_TOPIC_EVENTS {
            return Err(unconfirmed());
        }
        self.proofs.push(event);
        Ok(())
    }
    async fn observe_current(&mut self, room: &Room) -> Result<(), MatrixRoomOperationError> {
        let read = room.get_state_event_static::<RoomTopicEventContent>().await;
        if let Some(RawSyncOrStrippedState::Sync(canonical)) =
            read.map_err(MatrixRoomOperationError::from_sdk_error)?
            && let Some(id) = canonical
                .get_field::<OwnedEventId>("event_id")
                .map_err(|_| unconfirmed())?
        {
            self.remember(id)?;
        }
        Ok(())
    }
    async fn observe_next(&mut self, room: &Room) -> Result<(), MatrixRoomOperationError> {
        // SDK emits this after committing both its state store and RoomInfo.
        // Drop all payloads before awaiting a local read; retain only proof IDs.
        let proof = {
            let read = self.updates.recv().await;
            let update = read.map_err(|_| unconfirmed())?;
            let RoomUpdate::Joined { updates, .. } = update else {
                return Err(unconfirmed());
            };
            match updates.state {
                State::After(events) => events
                    .iter()
                    .rev()
                    .find(|event| {
                        event.get_field::<String>("type").ok().flatten().as_deref()
                            == Some("m.room.topic")
                            && event
                                .get_field::<String>("state_key")
                                .ok()
                                .flatten()
                                .as_deref()
                                == Some("")
                    })
                    .and_then(|event| event.get_field::<OwnedEventId>("event_id").ok().flatten()),
                // Ordinary /sync Before state precedes its timeline. Only the
                // resulting current SDK state can prove that batch's topic.
                State::Before(_) => None,
            }
        };
        if let Some(id) = proof {
            self.remember(id)?;
        }
        self.observe_current(room).await
    }
    async fn wait_for(
        &mut self,
        room: &Room,
        expected: &EventId,
    ) -> Result<(), MatrixRoomOperationError> {
        // Handles already committed/idempotent acknowledgements without needing
        // a repeated typed event or another sync response.
        self.observe_current(room).await?;
        while !self.proofs.iter().any(|id| id == expected) {
            self.observe_next(room).await?;
        }
        Ok(())
    }
}

pub(super) async fn update_topic(
    room: &Room,
    topic: &str,
) -> Result<MatrixRoomSettingsSnapshot, MatrixRoomOperationError> {
    update_topic_with_deadline(room, topic, TOPIC_WRITE_DEADLINE).await
}

async fn update_topic_with_deadline(
    room: &Room,
    topic: &str,
    deadline: std::time::Duration,
) -> Result<MatrixRoomSettingsSnapshot, MatrixRoomOperationError> {
    // Register before sending and consume commits while HTTP is pending. No
    // detached task or typed-event callback owns any part of this operation.
    let mut barrier = TopicCommitBarrier::new(room);
    tokio::time::timeout(deadline, async {
        let mut send = Box::pin(room.set_room_topic(topic));
        let response = loop {
            tokio::select! {
                response = send.as_mut() => break response.map_err(MatrixRoomOperationError::from_sdk_error)?,
                observed = barrier.observe_next(room) => observed?,
            }
        };
        barrier.wait_for(room, &response.event_id).await?;
        // A later real remote write may supersede an earlier proved commit.
        Ok(crate::room_projection::matrix_room_settings_snapshot(room).await)
    }).await.map_err(|_| unconfirmed()).and_then(|result| result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::{
        ruma::{
            api::client::sync::sync_events::v5 as sliding, event_id, events::StateEventType,
            room_id,
        },
        test_utils::mocks::MatrixMockServer,
    };
    use matrix_sdk_test::event_factory::EventFactory;

    async fn room() -> (MatrixMockServer, Room) {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        let room = server
            .sync_joined_room(&client, room_id!("!topic-owner:example.org"))
            .await;
        (server, room)
    }
    async fn commit(room: &Room, id: &EventId, topic: &str) {
        let factory = EventFactory::new().sender(room.own_user_id());
        let mut response = sliding::Response::new(id.to_string());
        let mut update = sliding::response::Room::new();
        update
            .required_state
            .push(factory.room_topic(topic).event_id(id).into_raw_sync_state());
        response.rooms.insert(room.room_id().to_owned(), update);
        room.client()
            .process_sliding_sync_test_helper(&response, &Default::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn early_committed_batch_proof_survives_remote_supersession_before_consumption() {
        let (_server, room) = room().await;
        let mut barrier = TopicCommitBarrier::new(&room);
        commit(&room, event_id!("$early-own:example.org"), "own").await;
        commit(&room, event_id!("$newer:example.org"), "newer").await;
        assert_eq!(room.topic().as_deref(), Some("newer"));
        barrier.observe_next(&room).await.unwrap();
        barrier
            .wait_for(&room, event_id!("$early-own:example.org"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn after_batch_uses_only_last_topic_entry() {
        let (_server, room) = room().await;
        let mut barrier = TopicCommitBarrier::new(&room);
        let factory = EventFactory::new().sender(room.own_user_id());
        let mut response = sliding::Response::new("same-batch".into());
        let mut update = sliding::response::Room::new();
        update.required_state.push(
            factory
                .room_topic("own")
                .event_id(event_id!("$superseded-in-batch:example.org"))
                .into_raw_sync_state(),
        );
        update.required_state.push(
            factory
                .room_topic("newer")
                .event_id(event_id!("$last-in-batch:example.org"))
                .into_raw_sync_state(),
        );
        response.rooms.insert(room.room_id().to_owned(), update);
        room.client()
            .process_sliding_sync_test_helper(&response, &Default::default())
            .await
            .unwrap();
        barrier.observe_next(&room).await.unwrap();
        assert!(
            !barrier
                .proofs
                .iter()
                .any(|id| id == event_id!("$superseded-in-batch:example.org"))
        );
        assert!(
            barrier
                .proofs
                .iter()
                .any(|id| id == event_id!("$last-in-batch:example.org"))
        );
    }

    #[tokio::test]
    async fn before_batch_cannot_prove_topic_superseded_by_its_timeline() {
        use matrix_sdk_test::JoinedRoomBuilder;
        let (server, room) = room().await;
        let mut barrier = TopicCommitBarrier::new(&room);
        let factory = EventFactory::new().sender(room.own_user_id());
        server
            .mock_sync()
            .ok_and_run(&room.client(), |builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room.room_id())
                        .add_state_event(
                            factory
                                .room_topic("own")
                                .event_id(event_id!("$before-timeline:example.org")),
                        )
                        .add_timeline_event(
                            factory
                                .room_topic("newer")
                                .event_id(event_id!("$timeline-current:example.org")),
                        ),
                );
            })
            .await;
        assert_eq!(room.topic().as_deref(), Some("newer"));
        barrier.observe_next(&room).await.unwrap();
        assert!(
            !barrier
                .proofs
                .iter()
                .any(|id| id == event_id!("$before-timeline:example.org"))
        );
        assert!(
            barrier
                .proofs
                .iter()
                .any(|id| id == event_id!("$timeline-current:example.org"))
        );
    }

    #[tokio::test]
    async fn aborting_commit_wait_releases_its_only_receiver() {
        let (_server, room) = room().await;
        let (sender, updates) = broadcast::channel(8);
        let mut barrier = TopicCommitBarrier {
            updates,
            proofs: Vec::new(),
        };
        let task = tokio::spawn(async move {
            barrier
                .wait_for(&room, event_id!("$not-seen:example.org"))
                .await
        });
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(sender.receiver_count(), 0);
    }

    #[tokio::test]
    async fn distinct_proofs_are_bounded_and_repeats_do_not_consume_capacity() {
        let (_server, room) = room().await;
        let mut barrier = TopicCommitBarrier::new(&room);
        for index in 0..MAX_EARLY_TOPIC_EVENTS {
            let id = format!("$capacity-{index}:example.org")
                .parse::<OwnedEventId>()
                .unwrap();
            commit(&room, &id, "synthetic").await;
            barrier.observe_next(&room).await.unwrap();
            barrier.observe_current(&room).await.unwrap();
            assert_eq!(barrier.proofs.len(), index + 1);
        }
        commit(&room, event_id!("$overflow:example.org"), "overflow").await;
        assert!(barrier.observe_next(&room).await.is_err());
        assert_eq!(barrier.proofs.len(), MAX_EARLY_TOPIC_EVENTS);
    }

    #[tokio::test]
    async fn sdk_broadcast_lag_fails_closed() {
        let (_server, room) = room().await;
        let mut barrier = TopicCommitBarrier::new(&room);
        for index in 0..9 {
            let id = format!("$lag-{index}:example.org")
                .parse::<OwnedEventId>()
                .unwrap();
            commit(&room, &id, "synthetic").await;
        }
        assert!(barrier.observe_next(&room).await.is_err());
        assert!(barrier.proofs.is_empty());
    }

    #[tokio::test]
    async fn acknowledged_but_unobserved_topic_expires_without_resending() {
        let (server, room) = room().await;
        let _http = server
            .mock_room_send_state()
            .for_type(StateEventType::RoomTopic)
            .ok(event_id!("$never-observed:example.org"))
            .mock_once()
            .mount_as_scoped()
            .await;
        assert!(
            update_topic_with_deadline(&room, "synthetic", std::time::Duration::from_millis(150))
                .await
                .is_err()
        );
        assert_eq!(room.topic(), None);
    }
}
