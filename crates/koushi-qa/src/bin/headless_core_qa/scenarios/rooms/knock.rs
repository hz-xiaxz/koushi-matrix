//! #1421: create a room whose join rule is `knock`, and have a second account
//! ask to join it.
//!
//! Koushi has no knock-to-join product path yet, so the second account knocks
//! through the SDK directly; the creator side only has to prove that Core
//! creates a room the homeserver accepts with the knock rule and that the
//! resulting request is accepted.

use super::super::scenario_identity::cleanup_qa_auditor_device;
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

    let knock = knock_with_second_account(config, &room_id).await;

    // A's room is disposable, but leave it explicitly even when the knock check
    // failed, so the lane does not accumulate memberships.
    let leave_id = conn_a.next_request_id();
    let leave = match conn_a
        .command(CoreCommand::Room(RoomCommand::LeaveRoom {
            request_id: leave_id,
            room_id: room_id.clone(),
        }))
        .await
    {
        Ok(()) => wait_for_room_left(conn_a, leave_id, &room_id, LABEL).await,
        Err(error) => Err(format!("{LABEL}: submit cleanup leave failed: {error}")),
    };
    knock?;
    println!("knock_join_request=ok");
    leave
}

async fn knock_with_second_account(config: &QaConfig, room_id: &str) -> Result<(), String> {
    let knocker = koushi_sdk::login_with_password(&LoginRequest {
        homeserver: config.homeserver.clone(),
        username: config.user_b.clone(),
        password: AuthSecret::new(config.password_b.clone()),
        device_display_name: Some("Koushi Knock QA".to_owned()),
    })
    .await
    .map_err(|_| format!("{LABEL}: second-account login failed"))?;
    let result = knock_and_assert(&knocker, config, room_id).await;
    let cleanup = cleanup_qa_auditor_device(&knocker, &config.password_b).await;
    let _ = koushi_sdk::close_session_stores(&knocker).await;
    drop(knocker);
    result?;
    cleanup.map_err(|error| format!("{LABEL}: {error}"))
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
    // knocking, so the accepted request is the proof that Core created a knock
    // room; the SDK then marks the returned room as knocked.
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
