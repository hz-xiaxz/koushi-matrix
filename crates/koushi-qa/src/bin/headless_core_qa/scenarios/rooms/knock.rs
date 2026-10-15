//! #1421: create a room whose join rule is `knock`, and have a second account
//! ask to join it.
//!
//! Koushi has no knock-to-join product path yet, so the second account knocks
//! through the SDK directly; the creator side only has to prove that Core
//! creates a room the homeserver accepts with the knock rule and that the
//! resulting membership is a knock.

use super::*;
use koushi_core::{CreateRoomOptions, CreateRoomVisibility};
use koushi_state::{AuthSecret, LoginRequest, RoomAccessPolicy, RoomJoinRule};

const LABEL: &str = "knock_create";

pub(super) async fn verify(config: &QaConfig, conn_a: &mut CoreConnection) -> Result<(), String> {
    let request_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::CreateRoom {
            request_id,
            options: CreateRoomOptions {
                name: "QA Knock Room".to_owned(),
                topic: None,
                alias_localpart: None,
                encrypted: false,
                invited_only: false,
                visibility: CreateRoomVisibility::Private,
                parent_space: None,
                access_policy: Some(RoomAccessPolicy::new(RoomJoinRule::Knock, Vec::new())),
                history: None,
            },
        }))
        .await
        .map_err(|error| format!("{LABEL}: submit create failed: {error}"))?;
    let room_id = wait_for_room_created(conn_a, request_id, LABEL).await?;
    println!("knock_room_created=ok");

    let knocker = koushi_sdk::login_with_password(&LoginRequest {
        homeserver: config.homeserver.clone(),
        username: config.user_b.clone(),
        password: AuthSecret::new(config.password_b.clone()),
        device_display_name: Some("Koushi Knock QA".to_owned()),
    })
    .await
    .map_err(|_| format!("{LABEL}: second-account login failed"))?;
    let result = knock_and_assert(&knocker, config, &room_id).await;
    let _ = koushi_sdk::close_session_stores(&knocker).await;
    drop(knocker);
    result?;
    println!("knock_join_request=ok");

    let leave_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::LeaveRoom {
            request_id: leave_id,
            room_id: room_id.clone(),
        }))
        .await
        .map_err(|error| format!("{LABEL}: submit cleanup leave failed: {error}"))?;
    wait_for_room_left(conn_a, leave_id, &room_id, LABEL).await?;
    Ok(())
}

async fn knock_and_assert(
    session: &koushi_sdk::MatrixClientSession,
    config: &QaConfig,
    room_id: &str,
) -> Result<(), String> {
    let room_or_alias = matrix_sdk::ruma::OwnedRoomOrAliasId::try_from(room_id.to_owned())
        .map_err(|_| format!("{LABEL}: room id is not addressable"))?;
    let via = vec![
        matrix_sdk::ruma::OwnedServerName::try_from(config.server_name.clone())
            .map_err(|_| format!("{LABEL}: server name is invalid"))?,
    ];
    // The knock request only succeeds when the server's join rule admits
    // knocking, and the SDK marks the returned room as knocked from that
    // accepted response; the local member event is not fetched yet.
    let knocked = session
        .client()
        .knock(room_or_alias, None, via)
        .await
        .map_err(|_| format!("{LABEL}: knock request failed"))?;
    if knocked.state() != matrix_sdk::RoomState::Knocked {
        return Err(format!(
            "{LABEL}: knocker state is {:?}, not knocked",
            knocked.state()
        ));
    }
    Ok(())
}
