//! Room-history export (#59) through `CoreCommand` against a local homeserver.
//!
//! Output is token-only. The exported files hold synthetic QA messages in a
//! per-run temporary directory and are deleted when the stage ends; no path,
//! identifier, or body is printed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use koushi_core::runtime::CoreConnection;
use koushi_protocol::command::{AccountCommand, CoreCommand, RoomHistoryExportRequest};
use koushi_protocol::ids::{AccountKey, RequestId, TimelineKey};
use koushi_state::{
    AppState, RoomHistoryExportProgress, RoomHistoryExportRange, RoomHistoryExportState,
};
use serde_json::Value;

use super::event_wait::{
    subscribe_timeline_for_qa, wait_for_encrypted_room_projection_for_qa,
    wait_for_invite_in_snapshot, wait_for_item_with_body, wait_for_send_flow_completion,
    wait_for_withheld_event_projection_from_source,
};
use super::fixtures::{accept_invite_for_qa, create_room_for_qa, invite_user_for_qa};
use super::registry::E2EE_EVENT_TIMEOUT;

const EXPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const HEADER_KEYS: [&str; 6] = [
    "room_name",
    "room_creator",
    "topic",
    "export_date",
    "exported_by",
    "messages",
];

/// A terminal export outcome read from the authoritative snapshot.
enum ExportOutcome {
    Completed(RoomHistoryExportProgress),
    Cancelled,
    Failed(String),
}

fn terminal_outcome(state: &AppState, request_id: RequestId) -> Option<ExportOutcome> {
    match &state.room_history_export {
        RoomHistoryExportState::Completed {
            request_id: id,
            progress,
            ..
        } if *id == request_id.sequence => Some(ExportOutcome::Completed(*progress)),
        RoomHistoryExportState::Cancelled { request_id: id, .. } if *id == request_id.sequence => {
            Some(ExportOutcome::Cancelled)
        }
        RoomHistoryExportState::Failed {
            request_id: id,
            failure_kind,
            ..
        } if *id == request_id.sequence => Some(ExportOutcome::Failed(format!("{failure_kind:?}"))),
        _ => None,
    }
}

async fn wait_for_terminal(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<ExportOutcome, String> {
    if let Some(outcome) = terminal_outcome(&conn.snapshot(), request_id) {
        return Ok(outcome);
    }
    tokio::time::timeout(EXPORT_TIMEOUT, async {
        loop {
            match conn.recv_event().await {
                Ok(_) => {}
                // A lagged stream still leaves the snapshot authoritative.
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
            if let Some(outcome) = terminal_outcome(&conn.snapshot(), request_id) {
                return Ok(outcome);
            }
        }
    })
    .await
    .map_err(|_| format!("{label}: timed out waiting for a terminal export state"))?
}

async fn start_export(
    conn: &mut CoreConnection,
    room_id: &str,
    range: RoomHistoryExportRange,
    destination: &Path,
    label: &str,
) -> Result<RequestId, String> {
    let request_id = conn.next_request_id();
    conn.register_native_artifact(
        request_id,
        koushi_core::NativeArtifactKind::RoomHistoryExportDestination,
        destination.to_path_buf(),
    )
    .map_err(|_| format!("{label}: register export destination"))?;
    conn.command(CoreCommand::Account(AccountCommand::ExportRoomHistory {
        request_id,
        request: RoomHistoryExportRequest {
            room_id: room_id.to_owned(),
            range,
            export_date_utc_offset_minutes: 0,
        },
    }))
    .await
    .map_err(|_| format!("{label}: submit export"))?;
    Ok(request_id)
}

async fn export_to_value(
    conn: &mut CoreConnection,
    room_id: &str,
    range: RoomHistoryExportRange,
    destination: &Path,
    label: &str,
) -> Result<(Value, RoomHistoryExportProgress), String> {
    let request_id = start_export(conn, room_id, range, destination, label).await?;
    let progress = match wait_for_terminal(conn, request_id, label).await? {
        ExportOutcome::Completed(progress) => progress,
        ExportOutcome::Cancelled => return Err(format!("{label}: export was cancelled")),
        ExportOutcome::Failed(kind) => return Err(format!("{label}: export failed kind={kind}")),
    };
    let bytes = std::fs::read(destination).map_err(|_| format!("{label}: read export file"))?;
    let value = serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| format!("{label}: export file is not JSON"))?;
    // Element's top-level key order, as `JSON.stringify` writes it.
    let text = String::from_utf8(bytes).map_err(|_| format!("{label}: export is not UTF-8"))?;
    let positions = HEADER_KEYS
        .iter()
        .map(|key| text.find(&format!("\n  \"{key}\": ")))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("{label}: export lacks an Element top-level key"))?;
    if !positions.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!("{label}: Element top-level key order differs"));
    }
    Ok((value, progress))
}

fn messages(value: &Value) -> &[Value] {
    value["messages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn event_ids(value: &Value) -> Vec<String> {
    messages(value)
        .iter()
        .filter_map(|event| event["event_id"].as_str().map(str::to_owned))
        .collect()
}

fn is_undecryptable(event: &Value) -> bool {
    event["type"] == "m.room.message" && event["content"]["msgtype"] == "m.bad.encrypted"
}

fn is_text_message(event: &Value) -> bool {
    event["type"] == "m.room.message" && event["content"]["msgtype"] == "m.text"
}

/// Structural checks shared by every completed export.
fn check_export(
    value: &Value,
    progress: &RoomHistoryExportProgress,
    label: &str,
) -> Result<(), String> {
    for key in [
        "room_name",
        "room_creator",
        "topic",
        "export_date",
        "exported_by",
    ] {
        if !value[key].is_string() {
            return Err(format!("{label}: header field {key} is not a string"));
        }
    }
    let events = messages(value);
    if events.len() as u64 != progress.exported_events {
        return Err(format!(
            "{label}: exported count {} differs from the file ({})",
            progress.exported_events,
            events.len()
        ));
    }
    let ids = event_ids(value);
    if ids.len() != events.len() || ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(format!(
            "{label}: exported event ids are missing or repeated"
        ));
    }
    for event in events {
        for field in ["type", "sender", "room_id", "event_id"] {
            if !event[field].is_string() {
                return Err(format!("{label}: exported event lacks {field}"));
            }
        }
        if !event["origin_server_ts"].is_u64() {
            return Err(format!("{label}: exported event lacks origin_server_ts"));
        }
        let kind = event["type"].as_str().unwrap_or_default();
        if matches!(kind, "m.reaction" | "m.room.redaction") {
            return Err(format!(
                "{label}: exported an event Element does not render"
            ));
        }
        if event["content"]["m.relates_to"]["rel_type"] == "m.replace" {
            return Err(format!("{label}: exported an edit event"));
        }
    }
    let undecryptable = events
        .iter()
        .filter(|event| is_undecryptable(event))
        .count() as u64;
    if undecryptable != progress.undecryptable_events {
        return Err(format!(
            "{label}: undecryptable count {} differs from the file ({undecryptable})",
            progress.undecryptable_events
        ));
    }
    Ok(())
}

fn no_partial_files(directory: &Path) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.filter_map(Result::ok).all(|entry| {
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".koushi-export-")
        })
    })
}

struct ExportDirectory(PathBuf);

impl Drop for ExportDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn send_text(
    conn: &mut CoreConnection,
    key: &TimelineKey,
    transaction_id: &str,
    body: &str,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Timeline(
        koushi_protocol::command::TimelineCommand::SendText {
            request_id,
            key: key.clone(),
            transaction_id: transaction_id.to_owned(),
            document: koushi_state::ComposerDocument::from_plain_text(body.to_owned()),
        },
    ))
    .await
    .map_err(|_| format!("{label}: submit send"))?;
    let outcome =
        wait_for_send_flow_completion(conn, request_id, key, transaction_id, body, label).await?;
    Ok(outcome.event_id)
}

/// Proofs (token-only):
/// - `history_export_full=ok`: a Core-owned export of an encrypted room walks
///   the server history forward from the first visible event and writes
///   Element's top-level object with every sent message decrypted, no edit,
///   reaction, or redaction events, and counts that match the file.
/// - `history_export_period=ok`: a period export contains exactly the events
///   of the full export whose timestamps fall in `[start, end)`.
/// - `history_export_utd_counted=ok`: a member whose device was denied a room
///   key exports that message as Element's `m.bad.encrypted` placeholder, and
///   the Rust-owned result counts it; the messages they can read stay
///   decrypted.
/// - `history_export_cancel=ok`: a cancelled export settles as cancelled and
///   leaves neither the destination nor a staging file.
pub(super) async fn run_room_history_export_stage(
    conn_a: &mut CoreConnection,
    account_key_a: &AccountKey,
    conn_b: &mut CoreConnection,
    account_key_b: &AccountKey,
) -> Result<(), String> {
    let directory = ExportDirectory(super::participants::qa_data_dir("history-export"));
    std::fs::create_dir_all(&directory.0)
        .map_err(|_| "history export: prepare export directory".to_owned())?;

    let room_id =
        create_room_for_qa(conn_a, "QA History Export", true, "history export room").await?;
    wait_for_encrypted_room_projection_for_qa(conn_a, &room_id, "history export room").await?;
    let key_a = TimelineKey::room(account_key_a.clone(), room_id.clone());
    subscribe_timeline_for_qa(conn_a, &key_a, "history export timeline").await?;

    let mut before_join = Vec::new();
    for index in 1..=3 {
        before_join.push(
            send_text(
                conn_a,
                &key_a,
                &format!("qa-history-export-before-{index}"),
                &format!("Synthetic history export message {index}"),
                "history export send before join",
            )
            .await?,
        );
    }
    invite_user_for_qa(conn_a, &room_id, &account_key_b.0, "history export invite").await?;
    wait_for_invite_in_snapshot(conn_b, &room_id, None, "history export invite").await?;
    accept_invite_for_qa(conn_b, &room_id, "history export join").await?;
    wait_for_encrypted_room_projection_for_qa(conn_b, &room_id, "history export join").await?;
    let key_b = TimelineKey::room(account_key_b.clone(), room_id.clone());
    let initial_b =
        subscribe_timeline_for_qa(conn_b, &key_b, "history export late member timeline").await?;
    let mut after_join = Vec::new();
    for index in 4..=5 {
        after_join.push(
            send_text(
                conn_a,
                &key_a,
                &format!("qa-history-export-after-{index}"),
                &format!("Synthetic history export message {index}"),
                "history export send after join",
            )
            .await?,
        );
    }
    // The later member holds the keys for messages sent after they joined
    // once the last one decrypts in their timeline.
    wait_for_item_with_body(
        conn_b,
        &key_b,
        "Synthetic history export message 5",
        "history export late member decrypt",
    )
    .await?;

    // A withholds the next room key from B's device, so B cannot decrypt it.
    let device_b = match &conn_b.snapshot().session {
        koushi_state::SessionState::Ready(info) => koushi_state::VerificationTarget {
            user_id: info.user_id.clone(),
            device_id: info.device_id.clone(),
        },
        _ => return Err("history export: late member is not Ready".to_owned()),
    };
    tokio::time::timeout(
        E2EE_EVENT_TIMEOUT,
        conn_a.qa_set_local_device_blacklisted(device_b, room_id.clone()),
    )
    .await
    .map_err(|_| "history export: block device ack timeout".to_owned())?
    .map_err(|_| "history export: block device failed".to_owned())?;
    let withheld_body = "Synthetic history export withheld message";
    let withheld = send_text(
        conn_a,
        &key_a,
        "qa-history-export-withheld",
        withheld_body,
        "history export withheld send",
    )
    .await?;
    wait_for_withheld_event_projection_from_source(
        conn_b,
        &key_b,
        &withheld,
        withheld_body,
        &initial_b,
        "history export withheld receive",
        E2EE_EVENT_TIMEOUT,
    )
    .await?;
    let sent = before_join.iter().chain(&after_join).collect::<Vec<_>>();

    // Full export by the room creator.
    let full_path = directory.0.join("full.json");
    let (full, full_progress) = export_to_value(
        conn_a,
        &room_id,
        RoomHistoryExportRange::AllAvailable,
        &full_path,
        "history export full",
    )
    .await?;
    check_export(&full, &full_progress, "history export full")?;
    let full_events = messages(&full);
    let sent_events = sent
        .iter()
        .map(|event_id| {
            full_events
                .iter()
                .find(|event| event["event_id"].as_str() == Some(event_id.as_str()))
                .ok_or_else(|| "history export full: a sent message is missing".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !sent_events.iter().all(|event| is_text_message(event)) {
        return Err("history export full: a sent message was not decrypted".to_owned());
    }
    if full_progress.undecryptable_events != 0 || full_progress.fetched_events < sent.len() as u64 {
        return Err("history export full: unexpected counts for the room creator".to_owned());
    }
    println!("history_export_full=ok");

    // Period export: [message 2, message 4).
    let start_ms = sent_events[1]["origin_server_ts"]
        .as_u64()
        .unwrap_or_default();
    let end_exclusive_ms = sent_events[3]["origin_server_ts"]
        .as_u64()
        .unwrap_or_default();
    if start_ms >= end_exclusive_ms {
        return Err("history export period: sent messages share a timestamp".to_owned());
    }
    let expected_period = full_events
        .iter()
        .filter(|event| {
            event["origin_server_ts"]
                .as_u64()
                .is_some_and(|ts| start_ms <= ts && ts < end_exclusive_ms)
        })
        .filter_map(|event| event["event_id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let period_path = directory.0.join("period.json");
    let (period, period_progress) = export_to_value(
        conn_a,
        &room_id,
        RoomHistoryExportRange::Period {
            start_ms,
            end_exclusive_ms,
            time_zone: "UTC".to_owned(),
        },
        &period_path,
        "history export period",
    )
    .await?;
    check_export(&period, &period_progress, "history export period")?;
    if event_ids(&period) != expected_period
        || !expected_period.contains(sent[1])
        || expected_period.contains(sent[3])
    {
        return Err("history export period: events differ from the full export range".to_owned());
    }
    println!("history_export_period=ok");

    // The later member cannot decrypt the withheld message.
    let late_path = directory.0.join("late-member.json");
    let (late, late_progress) = export_to_value(
        conn_b,
        &room_id,
        RoomHistoryExportRange::AllAvailable,
        &late_path,
        "history export late member",
    )
    .await?;
    check_export(&late, &late_progress, "history export late member")?;
    let late_events = messages(&late);
    let find = |event_id: &String| {
        late_events
            .iter()
            .find(|event| event["event_id"].as_str() == Some(event_id.as_str()))
    };
    let readable = before_join
        .iter()
        .chain(&after_join)
        .all(|event_id| find(event_id).is_some_and(is_text_message));
    let withheld_undecryptable = find(&withheld).is_some_and(is_undecryptable);
    if !readable || !withheld_undecryptable || late_progress.undecryptable_events == 0 {
        return Err(format!(
            "history export late member: unexpected decryption outcome \
             (readable={readable} withheld_undecryptable={withheld_undecryptable} \
             undecryptable_total={})",
            late_progress.undecryptable_events
        ));
    }
    println!("history_export_utd_counted=ok");

    // Cancellation. A tiny room can finish before the cancel reaches the
    // actor, so retry a bounded number of times.
    let cancel_path = directory.0.join("cancelled.json");
    let mut cancelled = false;
    for attempt in 0..3 {
        let _ = std::fs::remove_file(&cancel_path);
        let export_id = start_export(
            conn_a,
            &room_id,
            RoomHistoryExportRange::AllAvailable,
            &cancel_path,
            "history export cancel",
        )
        .await?;
        let cancel_id = conn_a.next_request_id();
        conn_a
            .command(CoreCommand::Account(
                AccountCommand::CancelRoomHistoryExport {
                    request_id: cancel_id,
                    target_request_id: export_id,
                },
            ))
            .await
            .map_err(|_| "history export cancel: submit cancel".to_owned())?;
        match wait_for_terminal(conn_a, export_id, "history export cancel").await? {
            ExportOutcome::Cancelled => {
                cancelled = true;
                break;
            }
            ExportOutcome::Completed(_) => {
                println!("history_export_cancel_race_attempt={attempt}");
            }
            ExportOutcome::Failed(kind) => {
                return Err(format!("history export cancel: export failed kind={kind}"));
            }
        }
    }
    if !cancelled {
        return Err("history export cancel: export completed before every cancel".to_owned());
    }
    if cancel_path.exists() || !no_partial_files(&directory.0) {
        return Err("history export cancel: a cancelled export left a file".to_owned());
    }
    println!("history_export_cancel=ok");
    println!("room_history_export=ok");
    Ok(())
}
