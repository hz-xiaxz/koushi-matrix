//! Parent-side Space child linking with SDK-derived routing (#1007).
//!
//! This mirrors `matrix_sdk_ui::spaces::SpaceService::add_child_to_space`
//! (write `m.space.child` routed by `Room::route()`, then the inverse
//! `m.space.parent` when the account may send it) without constructing a
//! `SpaceService`, whose constructor starts room-list subscriptions that the
//! Room actor would then have to own. Routing never derives from the room ID:
//! room version 12 IDs have no server component.

use super::{MatrixRoomOperationError, MatrixRoomOperationFailureKind};
use crate::MatrixClientSession;
use crate::room_projection::matrix_room;
use matrix_sdk::{
    deserialized_responses::{RawSyncOrStrippedState, SyncOrStrippedState},
    ruma::{
        OwnedServerName,
        events::{
            StateEventType, SyncStateEvent,
            space::{child::SpaceChildEventContent, parent::SpaceParentEventContent},
        },
    },
};

/// What happened to the child-side `m.space.parent` after the parent-side
/// `m.space.child` was written or found. The parent-side child event is the
/// Space relationship that other clients read; the inverse is advisory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixSpaceParentLinkOutcome {
    Written,
    AlreadyPresent,
    NotPermitted,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatrixSpaceChildLinkOutcome {
    /// `false` when a valid parent-side child event was already present, so no
    /// write was needed (a duplicate or retried submission).
    pub child_written: bool,
    pub parent: MatrixSpaceParentLinkOutcome,
}

/// Link a joined room under a joined Space.
///
/// Fails with `Forbidden` before any request when the Space's power levels do
/// not allow `m.space.child`; the homeserver remains the final authority.
pub async fn set_space_child(
    session: &MatrixClientSession,
    space_id: &str,
    child_room_id: &str,
) -> Result<MatrixSpaceChildLinkOutcome, MatrixRoomOperationError> {
    let space = matrix_room(session, space_id)?;
    let child = matrix_room(session, child_room_id)?;
    let user_id = session
        .client()
        .user_id()
        .ok_or(MatrixRoomOperationError::Sdk(
            MatrixRoomOperationFailureKind::AuthenticationRequired,
        ))?
        .to_owned();

    if let Ok(power_levels) = space.power_levels().await
        && !power_levels.user_can_send_state(&user_id, StateEventType::SpaceChild)
    {
        return Err(MatrixRoomOperationError::Sdk(
            MatrixRoomOperationFailureKind::Forbidden,
        ));
    }

    let child_written = if has_routed_space_child(&space, child.room_id()).await {
        false
    } else {
        let via = routing_servers(session, &child).await?;
        space
            .send_state_event_for_key(child.room_id(), SpaceChildEventContent::new(via))
            .await
            .map_err(MatrixRoomOperationError::from_sdk_error)?;
        true
    };

    let parent = link_space_parent(session, &space, &child, &user_id).await;
    Ok(MatrixSpaceChildLinkOutcome {
        child_written,
        parent,
    })
}

/// Issue #1410: mark or unmark a linked child of `space_id` as suggested.
///
/// Mirrors Element Web's Space hierarchy "Mark as suggested": the existing
/// `m.space.child` content is rewritten with only `suggested` changed, so
/// `via`, `order` and any field this client does not model survive. A pair
/// that is not linked (no routed child event) is `WrongRoomState`: toggling
/// must never create a relationship. Returns `false` when the event already
/// says what was asked and nothing was sent.
pub async fn set_space_child_suggested(
    session: &MatrixClientSession,
    space_id: &str,
    child_room_id: &str,
    suggested: bool,
) -> Result<bool, MatrixRoomOperationError> {
    let space = matrix_room(session, space_id)?;
    let child_room_id = matrix_sdk::ruma::RoomId::parse(child_room_id)
        .map_err(|_| MatrixRoomOperationError::InvalidRoomId)?;
    let user_id = session
        .client()
        .user_id()
        .ok_or(MatrixRoomOperationError::Sdk(
            MatrixRoomOperationFailureKind::AuthenticationRequired,
        ))?
        .to_owned();
    if !space_child_events_permitted(&space, &user_id).await {
        return Err(MatrixRoomOperationError::Sdk(
            MatrixRoomOperationFailureKind::Forbidden,
        ));
    }

    let Some(mut content) = routed_space_child_content(&space, &child_room_id).await else {
        return Err(MatrixRoomOperationError::Sdk(
            MatrixRoomOperationFailureKind::WrongRoomState,
        ));
    };
    let current = content
        .get("suggested")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if current == suggested {
        return Ok(false);
    }
    content.insert("suggested".to_owned(), serde_json::Value::Bool(suggested));
    space
        .send_state_event_raw(
            StateEventType::SpaceChild.to_string().as_str(),
            child_room_id.as_str(),
            serde_json::Value::Object(content),
        )
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(true)
}

/// Whether the account may send `m.space.child` in `space` (#1410). An
/// unreadable power-levels event is not a denial: the homeserver decides.
pub(crate) async fn space_child_events_permitted(
    space: &matrix_sdk::Room,
    user_id: &matrix_sdk::ruma::UserId,
) -> bool {
    match space.power_levels().await {
        Ok(power_levels) => power_levels.user_can_send_state(user_id, StateEventType::SpaceChild),
        Err(_) => true,
    }
}

/// The raw content of a routed (non-empty `via`) `m.space.child` event, every
/// field kept.
async fn routed_space_child_content(
    space: &matrix_sdk::Room,
    child_room_id: &matrix_sdk::ruma::RoomId,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let raw = space
        .get_state_event_static_for_key::<SpaceChildEventContent, _>(child_room_id)
        .await
        .ok()??;
    let RawSyncOrStrippedState::Sync(raw) = raw else {
        return None;
    };
    let content = raw
        .get_field::<serde_json::Map<String, serde_json::Value>>("content")
        .ok()??;
    let routed = content
        .get("via")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|via| !via.is_empty());
    routed.then_some(content)
}

async fn link_space_parent(
    session: &MatrixClientSession,
    space: &matrix_sdk::Room,
    child: &matrix_sdk::Room,
    user_id: &matrix_sdk::ruma::UserId,
) -> MatrixSpaceParentLinkOutcome {
    if has_space_parent(child, space.room_id()).await {
        // Preserve an existing parent event (its `canonical` flag included).
        return MatrixSpaceParentLinkOutcome::AlreadyPresent;
    }
    match child.power_levels().await {
        Ok(power_levels)
            if power_levels.user_can_send_state(user_id, StateEventType::SpaceParent) => {}
        Ok(_) => return MatrixSpaceParentLinkOutcome::NotPermitted,
        Err(_) => return MatrixSpaceParentLinkOutcome::Failed,
    }
    let Ok(via) = routing_servers(session, space).await else {
        return MatrixSpaceParentLinkOutcome::Failed;
    };
    match child
        .send_state_event_for_key(space.room_id(), SpaceParentEventContent::new(via))
        .await
    {
        Ok(_) => MatrixSpaceParentLinkOutcome::Written,
        Err(_) => MatrixSpaceParentLinkOutcome::Failed,
    }
}

/// Servers for a Space relationship's `via`: the SDK's routing algorithm over
/// the synced joined members. A room that has just been created (or whose
/// member list has not been loaded yet under sliding sync) may have no local
/// members; the session's own homeserver is then a valid route, because this
/// account is joined to the room through it. Never parsed from the room ID.
pub(crate) async fn routing_servers(
    session: &MatrixClientSession,
    room: &matrix_sdk::Room,
) -> Result<Vec<OwnedServerName>, MatrixRoomOperationError> {
    let route = room
        .route()
        .await
        .map_err(MatrixRoomOperationError::from_sdk_error)?;
    Ok(route_or_own_server(
        route,
        session.client().user_id().map(|id| id.server_name()),
    ))
}

pub(crate) fn route_or_own_server(
    route: Vec<OwnedServerName>,
    own_server: Option<&matrix_sdk::ruma::ServerName>,
) -> Vec<OwnedServerName> {
    if route.is_empty() {
        own_server.map(ToOwned::to_owned).into_iter().collect()
    } else {
        route
    }
}

async fn has_routed_space_child(
    space: &matrix_sdk::Room,
    child_room_id: &matrix_sdk::ruma::RoomId,
) -> bool {
    match space
        .get_state_event_static_for_key::<SpaceChildEventContent, _>(child_room_id)
        .await
    {
        Ok(Some(raw)) => match raw.deserialize() {
            Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(event))) => {
                !event.content.via.is_empty()
            }
            Ok(SyncOrStrippedState::Stripped(event)) => {
                event.content.via.is_some_and(|via| !via.is_empty())
            }
            _ => false,
        },
        _ => false,
    }
}

async fn has_space_parent(child: &matrix_sdk::Room, space_id: &matrix_sdk::ruma::RoomId) -> bool {
    match child
        .get_state_event_static_for_key::<SpaceParentEventContent, _>(space_id)
        .await
    {
        Ok(Some(raw)) => match raw.deserialize() {
            Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(event))) => {
                !event.content.via.is_empty()
            }
            Ok(SyncOrStrippedState::Stripped(event)) => {
                event.content.via.is_some_and(|via| !via.is_empty())
            }
            _ => false,
        },
        _ => false,
    }
}
