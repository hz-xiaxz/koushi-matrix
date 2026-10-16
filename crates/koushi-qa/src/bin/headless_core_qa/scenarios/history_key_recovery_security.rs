//! Security refusals and crash-lost-chunk recovery on disposable Synapse.
use super::*;
use koushi_state::HistoryKeyRecoveryFailure as Failure;
use matrix_sdk::encryption::LocalTrust;
use sha2::{Digest, Sha256};

async fn no_keys(participant: &Participant, history: &HistoricalRoom) -> Result<bool, String> {
    let client = participant.session().client();
    let machine = client.olm_machine_for_testing().await;
    Ok(machine
        .as_ref()
        .ok_or("history recovery: crypto unavailable")?
        .store()
        .get_inbound_group_sessions_by_room_id(&history.room_id)
        .await
        .map_err(|_| "history recovery: key inventory failed")?
        .is_empty())
}
async fn rejected(core: &CoreFixture, history: &HistoricalRoom, before: u64) -> Result<(), String> {
    wait_until("security refusal", || async {
        let metrics = core
            .runtime
            .as_ref()
            .ok_or("history recovery: runtime missing")?
            .history_recovery_diagnostics_for_testing(Some(history.room_id.to_string()))
            .await
            .ok_or("history recovery: diagnostics unavailable")?;
        Ok(metrics.rejected_messages > before)
    })
    .await
}
async fn expect_request_failure(
    core: &CoreFixture,
    history: &HistoricalRoom,
    reason: Failure,
) -> Result<(), String> {
    let previous = core
        .conn()
        .snapshot()
        .room_management
        .history_keys
        .get(history.room_id.as_str())
        .map_or(0, |s| s.generation);
    let request_id = core.conn().next_request_id();
    core.conn()
        .command(CoreCommand::Room(RoomCommand::RequestHistoryKeys {
            request_id,
            room_id: history.room_id.to_string(),
            force_all: false,
        }))
        .await
        .map_err(|_| "history recovery: refusal command rejected")?;
    wait_until("guarded command failure", || async {
        Ok(core
            .conn()
            .snapshot()
            .room_management
            .history_keys
            .get(history.room_id.as_str())
            .is_some_and(|s| {
                s.generation > previous
                    && matches!(s.status, Status::Failed { reason: actual } if actual == reason)
            }))
    })
    .await
}

pub(super) async fn run(
    sender: &mut Participant,
    recipient: &mut Participant,
    a: &mut CoreFixture,
    b: &mut CoreFixture,
) -> Result<(), String> {
    // Existing shared keys must not override the room's CURRENT joined-only
    // history rule, even when the requester can read encrypted event bodies.
    let restricted = public_history(sender).await?;
    sender
        .session()
        .client()
        .get_room(&restricted.room_id)
        .ok_or("history recovery: restricted room missing")?
        .send_state_event(RoomHistoryVisibilityEventContent::new(
            HistoryVisibility::Joined,
        ))
        .await
        .map_err(|_| "history recovery: visibility change failed")?;
    join(recipient, &restricted.room_id).await?;
    expect_request_failure(b, &restricted, Failure::HistoryNotShared).await?;
    if !no_keys(recipient, &restricted).await? {
        return Err("history recovery: restricted history keys disclosed".to_owned());
    }
    println!("history_recovery_visibility_guard=ok");

    // A queued legitimate request must not disclose keys after the requester
    // leaves, even though it was eligible at dispatch time.
    let left = public_history(sender).await?;
    a.sync(false).await?;
    join(recipient, &left.room_id).await?;
    b.request(&left, false, 1).await?;
    recipient
        .session()
        .client()
        .get_room(&left.room_id)
        .ok_or("history recovery: leave fixture missing")?
        .leave()
        .await
        .map_err(|_| "history recovery: leave failed")?;
    a.sync(true).await?;
    rejected(a, &left, 0).await?;
    if !no_keys(recipient, &left).await? {
        return Err("history recovery: keys disclosed after leave".to_owned());
    }
    println!("history_recovery_membership_guard=ok");

    // Pause AFTER identity refresh. A leave during that await must invalidate
    // disclosure through the final authoritative policy refresh.
    let race = public_history(sender).await?;
    let (reached, resume) = a
        .runtime
        .as_ref()
        .ok_or("history recovery: runtime missing")?
        .pause_next_history_key_disclosure_for_testing()
        .await
        .ok_or("history recovery: disclosure pause unavailable")?;
    join(recipient, &race.room_id).await?;
    b.request(&race, false, 1).await?;
    tokio::time::timeout(WAIT, reached)
        .await
        .map_err(|_| "history recovery: disclosure pause timed out")?
        .map_err(|_| "history recovery: disclosure pause cancelled")?;
    recipient
        .session()
        .client()
        .get_room(&race.room_id)
        .ok_or("history recovery: race room missing")?
        .leave()
        .await
        .map_err(|_| "history recovery: race leave failed")?;
    let _ = resume.send(());
    rejected(a, &race, 0).await?;
    if !no_keys(recipient, &race).await? {
        return Err("history recovery: stale policy disclosed keys".to_owned());
    }
    println!("history_recovery_policy_race_guard=ok");

    let blocked = public_history(sender).await?;
    let recipient_client = recipient.session().client();
    let source_client = sender.session().client();
    let device = source_client
        .encryption()
        .get_device(
            recipient_client
                .user_id()
                .ok_or("history recovery: requester missing")?,
            recipient_client
                .device_id()
                .ok_or("history recovery: requester device missing")?,
        )
        .await
        .map_err(|_| "history recovery: device lookup failed")?
        .ok_or("history recovery: device unavailable")?;
    device
        .set_local_trust(LocalTrust::BlackListed)
        .await
        .map_err(|_| "history recovery: device block failed")?;
    let blocked_result = async {
        join(recipient, &blocked.room_id).await?;
        b.request(&blocked, false, 1).await?;
        rejected(a, &blocked, 0).await?;
        if !no_keys(recipient, &blocked).await? {
            return Err("history recovery: keys disclosed to blocked device".to_owned());
        }
        Ok(())
    }
    .await;
    device
        .set_local_trust(LocalTrust::Unset)
        .await
        .map_err(|_| "history recovery: device unblock failed")?;
    blocked_result?;
    println!("history_recovery_blocked_device_guard=ok");
    drop(device);
    drop(source_client);
    drop(recipient_client);

    // Crash after durable chunk reservation, before the actual SDK import.
    // The restarted requester must accept the IDENTICAL cached response; a
    // regenerated response ID would fail its existing receipt validation.
    let crash = public_history(sender).await?;
    a.sync(false).await?;
    let (reached, resume) = b
        .runtime
        .as_ref()
        .ok_or("history recovery: runtime missing")?
        .pause_next_history_key_import_for_testing()
        .await
        .ok_or("history recovery: import pause unavailable")?;
    join(recipient, &crash.room_id).await?;
    b.request(&crash, false, 1).await?;
    a.sync(true).await?;
    tokio::time::timeout(WAIT, reached)
        .await
        .map_err(|_| "history recovery: import pause timed out")?
        .map_err(|_| "history recovery: import pause cancelled")?;
    if !no_keys(recipient, &crash).await? {
        return Err("history recovery: import pause was too late".to_owned());
    }
    let source_saved = ledger(&sender.session().client()).await?;
    let response = source_saved["cached"]
        .as_object()
        .ok_or("history recovery: response cache missing")?
        .values()
        .find(|c| c["room"] == crash.room_id.as_str())
        .ok_or("history recovery: pending cached response missing")?["response"]
        .clone();
    if response.as_str().is_none_or(str::is_empty) {
        return Err("history recovery: response metadata not durable".to_owned());
    }
    b.stop().await?;
    drop(resume);
    a.stop().await?;
    sender.restart_with_sync(false).await?;
    recipient.restart_with_sync(false).await?;
    // Simulate elapsed rate-limit time only; preserve the exact encrypted
    // payload, peer pins, response metadata, nonces and byte reservations.
    let mut source_saved = ledger(&sender.session().client()).await?;
    for time in source_saved["responded"]
        .as_object_mut()
        .ok_or("history recovery: responder cooldown missing")?
        .values_mut()
    {
        *time = serde_json::json!(0);
    }
    sender
        .session()
        .client()
        .encryption()
        .set_application_crypto_data(
            "chat.koushi.history_keys.v1",
            &serde_json::to_vec(&source_saved)
                .map_err(|_| "history recovery: cooldown encoding failed")?,
        )
        .await
        .map_err(|_| "history recovery: cooldown fixture failed")?;
    *a = CoreFixture::start(sender).await?;
    *b = CoreFixture::start(recipient).await?;
    let client = recipient.session().client();
    wait_until("crash-lost chunk recovery", || {
        historical_messages_decrypt(&client, &crash)
    })
    .await?;
    wait_until("durable acknowledgement and cache cleanup", || async {
        let metrics = a
            .runtime
            .as_ref()
            .ok_or("history recovery: runtime missing")?
            .history_recovery_barrier_for_testing()
            .await
            .ok_or("history recovery: diagnostics unavailable")?;
        let saved = ledger(&sender.session().client()).await?;
        let retired = saved["cached"].as_object().is_some_and(|entries| {
            entries.values().any(|c| {
                c["room"] == crash.room_id.as_str()
                    && c["response"] == response
                    && c["state"] == "Tombstone"
                    && c["bytes"] == 0
            })
        });
        // The earlier leave-during-disclosure room may retain its unacknowledged
        // bounded cache until expiry. Require cleanup of THIS response exactly.
        Ok(metrics.acknowledged_responses >= 1 && metrics.replayed_responses >= 1 && retired)
    })
    .await?;
    let saved = ledger(&sender.session().client()).await?;
    let cached = saved["cached"]
        .as_object()
        .ok_or("history recovery: completed cache missing")?
        .values()
        .find(|c| c["room"] == crash.room_id.as_str())
        .ok_or("history recovery: response tombstone missing")?;
    if cached["response"] != response || cached["state"] != "Tombstone" || cached["bytes"] != 0 {
        return Err("history recovery: cached response regenerated or not retired".to_owned());
    }
    let pin = &cached["pin"];
    let identity = format!(
        "{}\n{}\n{}",
        pin["user"]
            .as_str()
            .ok_or("history recovery: cache peer missing")?,
        pin["device"]
            .as_str()
            .ok_or("history recovery: cache device missing")?,
        cached["nonce"]
            .as_str()
            .ok_or("history recovery: cache correlation missing")?
    );
    let key = format!(
        "chat.koushi.history_keys.reply.{:x}",
        Sha256::digest(identity)
    );
    if sender
        .session()
        .client()
        .encryption()
        .application_crypto_data(&key)
        .await
        .map_err(|_| "history recovery: retired payload lookup failed")?
        .is_some()
    {
        return Err("history recovery: retired payload row retained".to_owned());
    }
    println!("history_recovery_crash_replay=ok");
    Ok(())
}
