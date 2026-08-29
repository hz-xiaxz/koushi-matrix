use super::actor::RoomActor;
use super::operations::{RoomOperationKind, classify_room_error, operation_failure_kind};
use crate::event::{CoreEvent, RoomEvent};
use crate::failure::CoreFailure;
use crate::ids::RequestId;
use koushi_sdk::{
    MatrixPreviewJoinability, MatrixPreviewMembership, MatrixPublicRoomDirectoryQuery,
    MatrixPublicRoomDirectoryRoom, MatrixRoomPreview,
};
use koushi_state::{
    AppAction, DirectoryPreviewJoinability, DirectoryPreviewMembership, DirectoryQuery,
    DirectoryRoomPreview, DirectoryRoomSummary, OperationFailureKind,
};

fn directory_room_summary_from_sdk(room: MatrixPublicRoomDirectoryRoom) -> DirectoryRoomSummary {
    DirectoryRoomSummary {
        room_id: room.room_id,
        canonical_alias: room.canonical_alias,
        room_type: room.room_type,
        name: room.name,
        topic: room.topic,
        avatar_url: room.avatar_url,
        joined_members: room.joined_members,
        world_readable: room.world_readable,
        guest_can_join: room.guest_can_join,
    }
}

fn directory_room_preview_from_sdk(preview: MatrixRoomPreview) -> DirectoryRoomPreview {
    DirectoryRoomPreview {
        room_id: preview.room_id,
        canonical_alias: preview.canonical_alias,
        room_type: preview.room_type,
        name: preview.name,
        topic: preview.topic,
        joined_members: preview.joined_members,
        joinability: match preview.joinability {
            MatrixPreviewJoinability::Open => DirectoryPreviewJoinability::Open,
            MatrixPreviewJoinability::InviteOnly => DirectoryPreviewJoinability::InviteOnly,
            MatrixPreviewJoinability::Restricted => DirectoryPreviewJoinability::Restricted,
            MatrixPreviewJoinability::Unknown => DirectoryPreviewJoinability::Unknown,
        },
        membership: match preview.membership {
            MatrixPreviewMembership::Joined => DirectoryPreviewMembership::Joined,
            MatrixPreviewMembership::Invited => DirectoryPreviewMembership::Invited,
            MatrixPreviewMembership::None => DirectoryPreviewMembership::None,
        },
    }
}

impl RoomActor {
    pub(super) async fn handle_create_public_directory_room(
        &self,
        request_id: RequestId,
        name: String,
        alias_localpart: String,
    ) {
        let Some(session) = &self.session else {
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::create_public_directory_room(session, &name, &alias_localpart).await {
            Ok(room_id) => {
                self.emit(CoreEvent::Room(RoomEvent::RoomCreated {
                    request_id,
                    room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_query_directory(
        &self,
        request_id: RequestId,
        query: DirectoryQuery,
    ) {
        self.reduce_reliable(vec![AppAction::DirectoryQueryRequested {
            request_id: request_id.sequence,
            query: query.clone(),
        }])
        .await;
        let Some(session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryQueryFailed {
                request_id: request_id.sequence,
                query,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let sdk_query = MatrixPublicRoomDirectoryQuery {
            term: query.term.clone(),
            server_name: query.server_name.clone(),
            limit: query.limit,
            since: query.since.clone(),
        };
        match koushi_sdk::query_public_room_directory(session, sdk_query).await {
            Ok(result) => {
                let rooms: Vec<DirectoryRoomSummary> = result
                    .rooms
                    .into_iter()
                    .map(directory_room_summary_from_sdk)
                    .collect();
                self.reduce_reliable(vec![AppAction::DirectoryQuerySucceeded {
                    request_id: request_id.sequence,
                    query: query.clone(),
                    rooms: rooms.clone(),
                    next_batch: result.next_batch.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::DirectoryQueryCompleted {
                    request_id,
                    query,
                    rooms,
                    next_batch: result.next_batch,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryQueryFailed {
                    request_id: request_id.sequence,
                    query,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_preview_join_target(
        &self,
        request_id: RequestId,
        room_id_or_alias: String,
        via_servers: Vec<String>,
    ) {
        self.reduce_reliable(vec![AppAction::DirectoryPreviewRequested {
            request_id: request_id.sequence,
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        }])
        .await;
        let Some(session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryPreviewFailed {
                request_id: request_id.sequence,
                room_id_or_alias,
                via_servers,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        let target = koushi_sdk::MatrixJoinTarget {
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        };
        match koushi_sdk::preview_join_target(session, &target).await {
            Ok(preview) => {
                let room = directory_room_preview_from_sdk(preview);
                self.reduce_reliable(vec![AppAction::DirectoryPreviewLoaded {
                    request_id: request_id.sequence,
                    room: room.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::DirectoryPreviewLoaded {
                    request_id,
                    room,
                }));
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryPreviewFailed {
                    request_id: request_id.sequence,
                    room_id_or_alias,
                    via_servers,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_join_directory_room(
        &self,
        request_id: RequestId,
        room_id_or_alias: String,
        via_servers: Vec<String>,
    ) {
        self.reduce_reliable(vec![AppAction::DirectoryJoinRequested {
            request_id: request_id.sequence,
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        }])
        .await;
        let Some(_session) = &self.session else {
            self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                request_id: request_id.sequence,
                room_id_or_alias,
                via_servers,
                kind: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };
        let operation = match self.begin_residency_operation() {
            Ok(operation) => operation,
            Err(()) => {
                self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                    request_id: request_id.sequence,
                    room_id_or_alias,
                    via_servers,
                    kind: OperationFailureKind::Sdk,
                }])
                .await;
                self.reject_residency_operation(request_id);
                return;
            }
        };

        let join_target = koushi_sdk::MatrixJoinTarget {
            room_id_or_alias: room_id_or_alias.clone(),
            via_servers: via_servers.clone(),
        };
        match self
            .call_room_operation(
                RoomOperationKind::JoinDirectoryRoom,
                koushi_sdk::join_room_target(&operation.session, &join_target),
            )
            .await
        {
            Ok(room_id) => {
                if !operation.room_rejoined(&room_id).await {
                    self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                        request_id: request_id.sequence,
                        room_id_or_alias,
                        via_servers,
                        kind: OperationFailureKind::Sdk,
                    }])
                    .await;
                    self.reject_residency_ack(request_id);
                    return;
                }
                self.reduce_reliable(vec![AppAction::DirectoryJoinSucceeded {
                    request_id: request_id.sequence,
                    room_id: room_id.clone(),
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::RoomJoined {
                    request_id,
                    room_id,
                }));
                self.refresh_room_list();
            }
            Err(error) => {
                let kind = classify_room_error(&error);
                self.reduce_reliable(vec![AppAction::DirectoryJoinFailed {
                    request_id: request_id.sequence,
                    room_id_or_alias,
                    via_servers,
                    kind: operation_failure_kind(kind),
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::RoomOperationFailed { kind });
            }
        }
    }
}

#[cfg(test)]
mod tests {}
