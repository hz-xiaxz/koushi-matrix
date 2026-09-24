//! Which rooms a Space export covers.
//!
//! The walk starts at the Space and descends into joined subspaces only
//! (`SpaceRoomList` reports direct children, so recursion is needed). Joined
//! non-DM rooms are exported; rooms the account has not joined, and unjoined
//! subspaces, are listed as skipped so the export shows what it could not
//! read. A visited set makes cyclic hierarchies terminate and lists a room
//! reachable from two subspaces once.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::sync::Arc;

/// One child a Space advertises.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChildEntry {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) joined: bool,
    pub(crate) is_space: bool,
}

/// The Space's children could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SelectionError;

pub(crate) trait SpaceChildSource: Send {
    fn children(
        &mut self,
        space_id: &str,
    ) -> impl Future<Output = Result<Vec<ChildEntry>, SelectionError>> + Send;
    fn is_dm(&mut self, room_id: &str) -> impl Future<Output = bool> + Send;
}

/// A room of a Space export. `target` is false for rooms and subspaces the
/// account has not joined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectedRoom {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) target: bool,
}

pub(crate) async fn select_space_rooms<S: SpaceChildSource>(
    source: &mut S,
    space_id: &str,
) -> Result<Vec<SelectedRoom>, SelectionError> {
    let mut selected = Vec::new();
    let mut listed = HashSet::new();
    let mut visited = HashSet::from([space_id.to_owned()]);
    let mut queue = VecDeque::from([space_id.to_owned()]);
    while let Some(current) = queue.pop_front() {
        let children = match source.children(&current).await {
            Ok(children) => children,
            Err(error) if current == space_id => return Err(error),
            // A subspace that cannot be read is left out; the rest continues.
            Err(_) => continue,
        };
        for child in children {
            if child.is_space && child.joined {
                if visited.insert(child.room_id.clone()) {
                    queue.push_back(child.room_id);
                }
                continue;
            }
            if !listed.insert(child.room_id.clone()) {
                continue;
            }
            if child.joined && !child.is_space && source.is_dm(&child.room_id).await {
                continue;
            }
            selected.push(SelectedRoom {
                room_id: child.room_id,
                display_name: child.display_name,
                target: child.joined,
            });
        }
    }
    Ok(selected)
}

/// Reads Space children and DM status through the Matrix SDK.
pub(crate) struct SdkSpaceChildSource {
    session: Arc<koushi_sdk::MatrixClientSession>,
}

impl SdkSpaceChildSource {
    pub(crate) fn new(session: Arc<koushi_sdk::MatrixClientSession>) -> Self {
        Self { session }
    }
}

impl SpaceChildSource for SdkSpaceChildSource {
    async fn children(&mut self, space_id: &str) -> Result<Vec<ChildEntry>, SelectionError> {
        let entries = koushi_sdk::matrix_space_children_projection(&self.session, space_id)
            .await
            .map_err(|_| SelectionError)?;
        Ok(entries
            .into_iter()
            .map(|entry| ChildEntry {
                joined: entry.membership == koushi_sdk::MatrixSpaceChildMembership::Joined,
                room_id: entry.room_id,
                display_name: entry.display_name,
                is_space: entry.is_space,
            })
            .collect())
    }

    async fn is_dm(&mut self, room_id: &str) -> bool {
        let Ok(room_id) = room_id.parse::<matrix_sdk::ruma::OwnedRoomId>() else {
            return false;
        };
        match self.session.client().get_room(&room_id) {
            Some(room) => koushi_sdk::matrix_room_is_dm(&room, None).await,
            None => false,
        }
    }
}
