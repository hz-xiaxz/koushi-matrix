//! Mock-homeserver coverage for the Space children projection: which count,
//! which join rule, and which name decide a child the account is not in
//! (#1053, #1062, #1070).

use matrix_sdk::{
    Client, RoomState,
    ruma::{
        OwnedRoomId, OwnedUserId, RoomId, RoomVersionId,
        events::{
            room::join_rules::{AllowRule, JoinRule, Restricted},
            space::child::SpaceChildEventContent,
        },
        owned_room_id, owned_user_id,
    },
    test_utils::mocks::MatrixMockServer,
};
use matrix_sdk_test::{JoinedRoomBuilder, LeftRoomBuilder, event_factory::EventFactory};
use wiremock::ResponseTemplate;

use super::{MatrixSpaceChildMembership, matrix_space_children_projection};
use crate::client_session::MatrixClientSession;

const SPACE_ID: &str = "!space:example.org";
const CHILD_ID: &str = "!child:example.org";

fn session(server: &MatrixMockServer, client: Client) -> MatrixClientSession {
    MatrixClientSession {
        info: koushi_state::SessionInfo {
            homeserver: server.server().uri(),
            user_id: client.user_id().unwrap().to_string(),
            device_id: client.device_id().unwrap().to_string(),
            authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
        },
        client,
        diagnostic_counters: koushi_diagnostics::DiagnosticCounterContext::registered(),
    }
}

async fn sync_space_advertising_child(server: &MatrixMockServer, client: &Client) {
    let own = client.user_id().unwrap().to_owned();
    let space_id = RoomId::parse(SPACE_ID).unwrap();
    let factory = EventFactory::new().room(&space_id).sender(&own);
    server
        .sync_room(
            client,
            JoinedRoomBuilder::new(&space_id)
                .add_state_event(factory.create(&own, RoomVersionId::V11).with_space_type())
                .add_state_event(factory.member(&own))
                .add_state_event(
                    factory
                        .event(SpaceChildEventContent::new(vec![
                            "example.org".try_into().unwrap(),
                        ]))
                        .state_key(CHILD_ID),
                ),
        )
        .await;
}

/// Sync the child as joined with `other` beside the account, then sync the
/// account's own leave. The local member list is frozen at that moment: it
/// still says `other` is joined, whatever happens in the room afterwards.
async fn sync_child_then_leave(
    server: &MatrixMockServer,
    client: &Client,
    join_rule: JoinRule,
    other: &OwnedUserId,
) {
    let own = client.user_id().unwrap().to_owned();
    let child_id: OwnedRoomId = RoomId::parse(CHILD_ID).unwrap();
    let factory = EventFactory::new().room(&child_id);
    server
        .sync_room(
            client,
            JoinedRoomBuilder::new(&child_id)
                .add_state_event(factory.create(&own, RoomVersionId::V11).sender(&own))
                .add_state_event(factory.member(&own).sender(&own))
                .add_state_event(factory.member(other).sender(other))
                .add_state_event(factory.room_join_rules(join_rule).sender(&own)),
        )
        .await;
    server
        .sync_room(
            client,
            LeftRoomBuilder::new(&child_id)
                .add_timeline_event(factory.member(&own).leave().sender(&own)),
        )
        .await;
    let child = client.get_room(&child_id).unwrap();
    assert_eq!(child.state(), RoomState::Left);
}

/// `/hierarchy` answering for the Space and, optionally, the child.
async fn mount_hierarchy(server: &MatrixMockServer, child: Option<serde_json::Value>) {
    let mut rooms = vec![serde_json::json!({
        "room_id": SPACE_ID,
        "room_type": "m.space",
        "num_joined_members": 1,
        "world_readable": false,
        "guest_can_join": false,
        "join_rule": "invite",
        "children_state": [],
    })];
    rooms.extend(child);
    server
        .mock_get_hierarchy()
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rooms": rooms,
        })))
        .mount()
        .await;
}

fn hierarchy_child(join_rule: &str, num_joined_members: u64) -> serde_json::Value {
    serde_json::json!({
        "room_id": CHILD_ID,
        "name": "Child",
        "num_joined_members": num_joined_members,
        "world_readable": false,
        "guest_can_join": false,
        "join_rule": join_rule,
        "children_state": [],
    })
}

fn knock_restricted() -> JoinRule {
    JoinRule::KnockRestricted(Restricted::new(vec![AllowRule::room_membership(
        owned_room_id!("!allowed:example.org"),
    )]))
}

/// #1062: after the account leaves, its local member list can never reach
/// zero on its own. The server's fresh count is the authority for a room the
/// account is no longer in, so an emptied child is not projected.
#[tokio::test]
async fn left_child_uses_the_hierarchy_count_over_the_frozen_local_member_list() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    sync_space_advertising_child(&server, &client).await;
    sync_child_then_leave(
        &server,
        &client,
        JoinRule::Public,
        &owned_user_id!("@bob:example.org"),
    )
    .await;
    mount_hierarchy(&server, Some(hierarchy_child("public", 0))).await;

    let entries = matrix_space_children_projection(&session(&server, client), SPACE_ID)
        .await
        .unwrap();

    assert!(
        entries.iter().all(|entry| entry.room_id != CHILD_ID),
        "an emptied child must not be projected: {entries:?}"
    );
}

/// #1062: a left child that still has members is still offered to rejoin.
#[tokio::test]
async fn left_child_with_members_is_still_projected_as_joinable() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    sync_space_advertising_child(&server, &client).await;
    sync_child_then_leave(
        &server,
        &client,
        JoinRule::Public,
        &owned_user_id!("@bob:example.org"),
    )
    .await;
    mount_hierarchy(&server, Some(hierarchy_child("public", 2))).await;

    let entries = matrix_space_children_projection(&session(&server, client), SPACE_ID)
        .await
        .unwrap();

    let child = entries
        .iter()
        .find(|entry| entry.room_id == CHILD_ID)
        .expect("a nonempty child stays in the Space");
    assert_eq!(child.membership, MatrixSpaceChildMembership::Left);
    assert_eq!(child.joined_members, 2);
    assert!(child.can_join);
}

/// #1053: a knock-restricted child is not offered a plain join when
/// `/hierarchy` describes it.
#[tokio::test]
async fn hierarchy_described_knock_restricted_child_cannot_be_joined() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    sync_space_advertising_child(&server, &client).await;
    mount_hierarchy(
        &server,
        Some(serde_json::json!({
            "room_id": CHILD_ID,
            "name": "Child",
            "num_joined_members": 2,
            "world_readable": false,
            "guest_can_join": false,
            "join_rule": "knock_restricted",
            "allowed_room_ids": ["!allowed:example.org"],
            "children_state": [],
        })),
    )
    .await;

    let entries = matrix_space_children_projection(&session(&server, client), SPACE_ID)
        .await
        .unwrap();

    let child = entries
        .iter()
        .find(|entry| entry.room_id == CHILD_ID)
        .expect("a nonempty child stays in the Space");
    assert_eq!(child.membership, MatrixSpaceChildMembership::NotJoined);
    assert!(!child.can_join);
}

/// #1053: the same knock-restricted child, known only locally because
/// `/hierarchy` omitted it, gets the same answer.
#[tokio::test]
async fn locally_known_knock_restricted_child_cannot_be_joined() {
    let server = MatrixMockServer::new().await;
    let client = server.client_builder().build().await;
    sync_space_advertising_child(&server, &client).await;
    sync_child_then_leave(
        &server,
        &client,
        knock_restricted(),
        &owned_user_id!("@bob:example.org"),
    )
    .await;
    mount_hierarchy(&server, None).await;

    let entries = matrix_space_children_projection(&session(&server, client), SPACE_ID)
        .await
        .unwrap();

    let child = entries
        .iter()
        .find(|entry| entry.room_id == CHILD_ID)
        .expect("the locally known child stays in the Space");
    assert_eq!(child.membership, MatrixSpaceChildMembership::Left);
    assert!(!child.can_join);
}

/// #1070: an unnamed hierarchy child with one member gets the SDK's English
/// calculated "Empty Room". It is marked structurally so the GUI renders its
/// own catalog text, while a room literally named "Empty Room" stays data.
#[tokio::test]
async fn hierarchy_child_with_a_calculated_empty_name_is_marked_as_a_placeholder() {
    for (name, expected) in [
        (None, Some(koushi_state::RoomNamePlaceholder::Empty)),
        (Some("Empty Room"), None),
    ] {
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().build().await;
        sync_space_advertising_child(&server, &client).await;
        let mut child = serde_json::json!({
            "room_id": CHILD_ID,
            "num_joined_members": 1,
            "world_readable": false,
            "guest_can_join": false,
            "join_rule": "public",
            "children_state": [],
        });
        if let Some(name) = name {
            child["name"] = serde_json::json!(name);
        }
        mount_hierarchy(&server, Some(child)).await;

        let entries = matrix_space_children_projection(&session(&server, client), SPACE_ID)
            .await
            .unwrap();

        let child = entries
            .iter()
            .find(|entry| entry.room_id == CHILD_ID)
            .expect("a child with a member is projected");
        assert_eq!(child.display_name, "Empty Room");
        assert_eq!(child.display_name_placeholder, expected, "name {name:?}");
    }
}
