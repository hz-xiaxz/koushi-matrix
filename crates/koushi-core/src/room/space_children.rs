//! Issue #961: loading a Space's advertised children.
//!
//! Mirrors the Space member load: the command is admitted against the selected
//! Space and its generation, the SDK projection runs, and the result is
//! reduced under the same generation so a slow response cannot paint another
//! Space's rooms.

use koushi_protocol::event::{CoreEvent, RoomEvent};
use koushi_protocol::failure::CoreFailure;
use koushi_protocol::ids::RequestId;
use koushi_state::{
    AppAction, OperationFailureKind, SpaceChildMembership, SpaceChildSummary,
};

use super::RoomActor;
use super::normalization::avatar_from_mxc_uri;
use koushi_sdk::{MatrixSpaceChildEntry, MatrixSpaceChildMembership};

impl RoomActor {
    pub(super) async fn handle_load_space_children(
        &mut self,
        request_id: RequestId,
        space_id: String,
        generation: u64,
    ) {
        let Some(session) = self.session.clone() else {
            self.reduce_reliable(vec![AppAction::SpaceChildrenLoadFailed {
                space_id,
                generation,
                failure: OperationFailureKind::Sdk,
            }])
            .await;
            self.emit_failure(request_id, CoreFailure::SessionRequired);
            return;
        };

        match koushi_sdk::matrix_space_children_projection(&session, &space_id).await {
            Ok(entries) => {
                let children: Vec<SpaceChildSummary> =
                    entries.iter().map(space_child_summary).collect();
                let child_count = children.len();
                self.reduce_reliable(vec![AppAction::SpaceChildrenLoaded {
                    space_id,
                    generation,
                    children,
                }])
                .await;
                self.emit(CoreEvent::Room(RoomEvent::SpaceChildrenLoaded {
                    request_id,
                    generation,
                    child_count,
                }));
            }
            Err(error) => {
                let failure = OperationFailureKind::Sdk;
                self.reduce_reliable(vec![AppAction::SpaceChildrenLoadFailed {
                    space_id,
                    generation,
                    failure,
                }])
                .await;
                self.emit_failure(
                    request_id,
                    CoreFailure::RoomOperationFailed {
                        kind: crate::room::operations::classify_room_error(&error),
                    },
                );
            }
        }
    }
}

fn space_child_summary(entry: &MatrixSpaceChildEntry) -> SpaceChildSummary {
    SpaceChildSummary {
        room_id: entry.room_id.clone(),
        display_name: entry.display_name.clone(),
        avatar: avatar_from_mxc_uri(entry.avatar_mxc_uri.as_deref()),
        membership: match entry.membership {
            MatrixSpaceChildMembership::Joined => SpaceChildMembership::Joined,
            MatrixSpaceChildMembership::Invited => SpaceChildMembership::Invited,
            MatrixSpaceChildMembership::Knocked => SpaceChildMembership::Knocked,
            MatrixSpaceChildMembership::Left => SpaceChildMembership::Left,
            MatrixSpaceChildMembership::Banned => SpaceChildMembership::Banned,
            MatrixSpaceChildMembership::NotJoined => SpaceChildMembership::NotJoined,
            MatrixSpaceChildMembership::Unknown => SpaceChildMembership::Unknown,
        },
        can_join: entry.can_join,
        is_space: entry.is_space,
        joined_members: entry.joined_members,
    }
}

