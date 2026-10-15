//! #1023: rooms created from a public Space on a real homeserver.
//!
//! The create-room defaults select a public room in a public Space, an unnamed
//! room omits `m.room.name`, an unnamed public room is rejected without an
//! address and created with the session's random default (#1416), whose
//! canonical alias then names it, a conflict on that address is reported, the
//! room can be named later, and explicitly choosing private still creates a
//! restricted Space room.

use super::*;
use koushi_core::{CreateRoomOptions, CreateRoomParentSpace, CreateRoomVisibility};
use koushi_state::{RoomJoinRule, SpaceChildLinkOutcome, compose_sidebar_for_state};

pub(super) async fn verify(conn_a: &mut CoreConnection) -> Result<(), String> {
    let space_id = create_space_for_qa(conn_a, "Koushi Open Space", "unnamed space create").await?;
    wait_for_space_in_space_list(conn_a, &space_id, "unnamed space list").await?;
    let update_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::UpdateRoomSetting {
            request_id: update_id,
            room_id: space_id.clone(),
            change: RoomSettingChange::JoinRule(RoomJoinRule::Public),
        }))
        .await
        .map_err(|_| "unnamed: Space join rule submission failed")?;
    wait_for_room_setting_updated(conn_a, update_id, "unnamed Space public").await?;
    wait_for_state(conn_a, "unnamed: public Space rule sync", |state| {
        state.spaces.iter().any(|space| {
            space.space_id == space_id && space.join_rule == Some(RoomJoinRule::Public)
        })
    })
    .await?;
    select_space(conn_a, Some(&space_id)).await?;
    let defaults = compose_sidebar_for_state(&conn_a.snapshot()).create_room_defaults;
    if defaults.visibility != CreateRoomVisibility::Public || !defaults.encrypted {
        return Err("unnamed: a public Space did not default to a public room".into());
    }
    println!("room_public_space_default=ok");

    let options = |alias_localpart: Option<String>| CreateRoomOptions {
        name: String::new(),
        topic: None,
        alias_localpart,
        encrypted: false,
        invited_only: false,
        visibility: CreateRoomVisibility::Public,
        parent_space: Some(CreateRoomParentSpace {
            space_id: space_id.clone(),
        }),
        access_policy: None,
        history: None,
    };

    // #1416: a public room always has an address, named or not.
    let rejected_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::CreateRoom {
            request_id: rejected_id,
            options: options(None),
        }))
        .await
        .map_err(|_| "unnamed: address-less submission failed")?;
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        match deadline
            .recv(conn_a)
            .await
            .map_err(|_| "unnamed: address-less outcome timeout")?
            .map_err(|_| "unnamed: address-less event stream lagged")?
        {
            CoreEvent::OperationFailed { request_id, .. } if request_id == rejected_id => break,
            CoreEvent::Room(RoomEvent::RoomCreated { request_id, .. })
                if request_id == rejected_id =>
            {
                return Err("unnamed: a public room was created without an address".into());
            }
            _ => {}
        }
    }

    // The session's default address: public, listed, linked, and the
    // canonical alias names the unnamed room (the SDK shows its localpart).
    let preview = open_create_draft_for_qa(conn_a, Some(&space_id), "unnamed default").await?;
    if !preview.localpart.starts_with("koushi-open-space-") {
        return Err("unnamed: the default address did not carry the Space prefix".into());
    }
    let alias_localpart = preview.localpart.clone();
    let unnamed_id = create(
        conn_a,
        options(Some(alias_localpart.clone())),
        "unnamed public",
    )
    .await?;
    wait_for_linked(conn_a, &space_id, &unnamed_id).await?;
    let settings = load_room_settings_for_qa(conn_a, &unnamed_id, "unnamed settings").await?;
    if settings.join_rule != RoomJoinRule::Public || settings.name.is_some() {
        return Err(format!(
            "unnamed: unexpected state (join_rule={:?}, has_name={})",
            settings.join_rule,
            settings.name.is_some()
        ));
    }
    println!("room_unnamed_public_space=ok");
    wait_for_display_name(conn_a, &unnamed_id, "unnamed alias name", |name| {
        name == alias_localpart
    })
    .await?;
    // Link acknowledgement and the room name can precede the Space's synced
    // child projection. Wait for the exact sidebar row, not a fixed delay.
    wait_for_state(conn_a, "unnamed: Space alias-name row", |state| {
        compose_sidebar_for_state(state)
            .space_rooms
            .iter()
            .any(|row| row.room_id == unnamed_id && row.display_name == alias_localpart)
    })
    .await?;
    println!("room_unnamed_alias_name=ok");

    let conflict_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::CreateRoom {
            request_id: conflict_id,
            options: options(Some(alias_localpart.clone())),
        }))
        .await
        .map_err(|_| "unnamed: conflict submission failed")?;
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        match deadline
            .recv(conn_a)
            .await
            .map_err(|_| "unnamed: conflict outcome timeout")?
            .map_err(|_| "unnamed: conflict event stream lagged")?
        {
            CoreEvent::OperationFailed {
                request_id,
                failure:
                    CoreFailure::RoomOperationFailed {
                        kind: RoomFailureKind::AliasInUse,
                    },
            } if request_id == conflict_id => break,
            CoreEvent::OperationFailed { request_id, .. } if request_id == conflict_id => {
                return Err("unnamed: the conflict did not report AliasInUse".into());
            }
            CoreEvent::Room(RoomEvent::RoomCreated { request_id, .. })
                if request_id == conflict_id =>
            {
                return Err("unnamed: a duplicate address was accepted".into());
            }
            _ => {}
        }
    }
    println!("room_unnamed_alias_conflict=ok");

    // Naming the room later replaces the calculated name.
    let rename_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::UpdateRoomSetting {
            request_id: rename_id,
            room_id: unnamed_id.clone(),
            change: RoomSettingChange::Name(Some("Koushi Named Later".to_owned())),
        }))
        .await
        .map_err(|_| "unnamed: rename submission failed")?;
    wait_for_room_setting_updated(conn_a, rename_id, "unnamed rename").await?;
    wait_for_display_name(conn_a, &unnamed_id, "unnamed renamed", |name| {
        name == "Koushi Named Later"
    })
    .await?;
    println!("room_unnamed_rename=ok");

    // Choosing private in a public Space keeps the Space-member default.
    let private_id = create(
        conn_a,
        CreateRoomOptions {
            name: "Koushi Private In Open Space".to_owned(),
            encrypted: true,
            visibility: CreateRoomVisibility::Private,
            ..options(None)
        },
        "public Space private choice",
    )
    .await?;
    wait_for_linked(conn_a, &space_id, &private_id).await?;
    let private = load_room_settings_for_qa(conn_a, &private_id, "private choice settings").await?;
    if private.join_rule != RoomJoinRule::Restricted || private.canonical_alias.is_some() {
        return Err(format!(
            "unnamed: the private choice was not a restricted room (join_rule={:?})",
            private.join_rule
        ));
    }
    println!("room_public_space_private_choice=ok");
    select_space(conn_a, None).await
}

async fn create(
    conn: &mut CoreConnection,
    options: CreateRoomOptions,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::CreateRoom {
        request_id,
        options,
    }))
    .await
    .map_err(|_| format!("{label}: create submission failed"))?;
    let room_id = wait_for_room_created(conn, request_id, label).await?;
    wait_for_room_in_room_list(conn, &room_id, label).await?;
    Ok(room_id)
}

async fn wait_for_linked(
    conn: &mut CoreConnection,
    space_id: &str,
    room_id: &str,
) -> Result<(), String> {
    let mut outcome = None;
    wait_for_state(conn, "unnamed: Space link settlement", |state| {
        outcome = state
            .space_child_links
            .latest(space_id, room_id)
            .map(|result| result.outcome);
        outcome.is_some()
    })
    .await?;
    match outcome {
        Some(SpaceChildLinkOutcome::Linked) => Ok(()),
        _ => Err("unnamed: the room was not linked to its Space".into()),
    }
}

async fn wait_for_display_name(
    conn: &mut CoreConnection,
    room_id: &str,
    label: &str,
    accept: impl Fn(&str) -> bool,
) -> Result<String, String> {
    let mut found = None;
    wait_for_state(conn, label, |state| {
        found = state
            .rooms
            .iter()
            .find(|room| room.room_id == room_id)
            .map(|room| room.display_name.clone())
            .filter(|name| accept(name));
        found.is_some()
    })
    .await?;
    found.ok_or_else(|| format!("{label}: display name missing"))
}

async fn wait_for_state(
    conn: &mut CoreConnection,
    label: &str,
    mut ready: impl FnMut(&AppState) -> bool,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    while !ready(&conn.snapshot()) {
        tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timeout"))?
            .map_err(|_| format!("{label}: event stream lagged"))?;
    }
    Ok(())
}

async fn select_space(conn: &mut CoreConnection, space_id: Option<&str>) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::SelectSpace {
        request_id,
        space_id: space_id.map(str::to_owned),
    }))
    .await
    .map_err(|_| "unnamed: select Space submission failed")?;
    wait_for_state(conn, "unnamed: select Space", |state| {
        state.navigation.active_space_id.as_deref() == space_id
    })
    .await
}
