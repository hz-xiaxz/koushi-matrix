//! Idle-sync gap-inspection benchmark (#1392).
//!
//! Reproduces the measured amplification: with resident room timelines and a
//! running sync, committed all-rooms responses (including zero-room ones)
//! schedule a `LiveEdge` gap inspection per omitted room. The scenario builds
//! several encrypted rooms with a large persisted history, keeps their
//! timelines resident, then observes an idle window and harvests the
//! content-free `core.timeline_gap_inspection` records:
//!
//! - a healthy room must report `event_bodies_decoded = 0` per inspection,
//! - inspection durations are summarized as p50/p95,
//! - the number of zero-room committed responses inside the window is
//!   reported (they are the amplifier; failing to observe any is a failure
//!   with a clear message telling the operator to widen the window).
//!
//! No room/event identifiers, message bodies, tokens, or local paths are ever
//! printed; the emitted values are duration and count aggregates only.

use koushi_diagnostics::{
    DiagnosticRecord,
    test_support::{DetailCursor, detail_records_since},
};
use koushi_protocol::ids::TimelineKey;

use super::cleanup::cleanup_logged_in_runtime;
use super::diagnostics::{diagnostic_count_field, diagnostic_token_field};
use super::event_wait::{
    send_text_expect_local_echo, subscribe_timeline_for_qa, wait_for_event_item_with_body,
    wait_for_logged_in, wait_for_ready_snapshot, wait_for_sync_started_and_running,
};
use super::fixtures::create_room_for_qa;
use super::participants::{complete_new_identity_gate_for_qa, qa_data_dir};
use super::registry::QaConfig;
use super::{
    AccountCommand, AccountKey, AuthSecret, CoreCommand, CoreRuntime, Duration, SyncCommand,
};

/// Default and bounded fixture dimensions, to keep the lane fast and
/// reproducible; override through the documented environment variables.
const DEFAULT_ROOM_COUNT: usize = 3;
const MAX_ROOM_COUNT: usize = 8;
const DEFAULT_MESSAGES_PER_ROOM: usize = 90;
const MAX_MESSAGES_PER_ROOM: usize = 500;
const DEFAULT_IDLE_WINDOW_MS: u64 = 45_000;
const MAX_IDLE_WINDOW_MS: u64 = 180_000;
/// Bounded settlement after a zero-room committed response so the LiveEdge
/// inspection it schedules can complete before harvesting.
const INSPECTION_SETTLEMENT_MS: u64 = 10_000;

const ENV_ROOM_COUNT: &str = "KOUSHI_QA_IDLE_INSPECTION_ROOMS";
const ENV_MESSAGES_PER_ROOM: &str = "KOUSHI_QA_IDLE_INSPECTION_MESSAGES_PER_ROOM";
const ENV_IDLE_WINDOW_MS: &str = "KOUSHI_QA_IDLE_INSPECTION_WINDOW_MS";

fn bounded_env_usize(name: &str, default: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.min(max))
        .unwrap_or(default)
}

fn bounded_env_u64(name: &str, default: u64, max: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|value| value.min(max))
        .unwrap_or(default)
}

fn gap_inspection_records_since(cursor: DetailCursor) -> Vec<DiagnosticRecord> {
    detail_records_since(cursor)
        .into_iter()
        .filter(|record| {
            record.event.source == "core.timeline_gap_inspection"
                && record.event.stage == "finished"
        })
        .collect()
}

/// Continuity of the inspected chain, carried inside the record itself
/// (generation is room-local and must not be used to correlate across
/// rooms). Only `complete` chains may decode zero event bodies.
fn continuity_of(record: &DiagnosticRecord) -> &str {
    diagnostic_token_field(&record.event, "continuity").unwrap_or("unknown")
}

fn committed_response_records_since(cursor: DetailCursor) -> Vec<DiagnosticRecord> {
    detail_records_since(cursor)
        .into_iter()
        .filter(|record| {
            record.event.source == "core.sync" && record.event.stage == "committed_response"
        })
        .collect()
}

fn p50_p95(mut samples: Vec<u64>) -> (u64, u64) {
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95) / 100];
    (p50, p95)
}

pub(super) async fn run_idle_sync_gap_inspection_scenario(config: &QaConfig) -> Result<(), String> {
    let room_count = bounded_env_usize(ENV_ROOM_COUNT, DEFAULT_ROOM_COUNT, MAX_ROOM_COUNT);
    let messages_per_room = bounded_env_usize(
        ENV_MESSAGES_PER_ROOM,
        DEFAULT_MESSAGES_PER_ROOM,
        MAX_MESSAGES_PER_ROOM,
    )
    .max(1);
    let idle_window_ms = bounded_env_u64(
        ENV_IDLE_WINDOW_MS,
        DEFAULT_IDLE_WINDOW_MS,
        MAX_IDLE_WINDOW_MS,
    );

    let data_dir_a = qa_data_dir("idle_sync_gap_inspection_a");
    let runtime_a = CoreRuntime::start_with_data_dir(data_dir_a.clone());
    let mut conn_a = runtime_a.attach();

    let login_a_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Account(AccountCommand::LoginPassword {
            request_id: login_a_id,
            request: koushi_state::LoginRequest {
                homeserver: config.homeserver.clone(),
                username: config.user_a.clone(),
                password: AuthSecret::new(config.password_a.clone()),
                device_display_name: Some("Koushi Core QA Idle Gap Inspection A".to_owned()),
            },
            platform: koushi_state::DisplayPlatform::Linux,
        }))
        .await
        .map_err(|e| format!("idle_sync_gap_inspection: submit login A failed: {e}"))?;
    complete_new_identity_gate_for_qa(&mut conn_a, &config.password_a).await?;

    let account_key_a: AccountKey =
        wait_for_logged_in(&mut conn_a, login_a_id, "idle_sync_gap_inspection login A").await?;
    wait_for_ready_snapshot(&mut conn_a, "idle_sync_gap_inspection session A Ready").await?;
    let sync_start_a_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Sync(SyncCommand::Start {
            request_id: sync_start_a_id,
        }))
        .await
        .map_err(|e| format!("idle_sync_gap_inspection: submit sync start A failed: {e}"))?;
    wait_for_sync_started_and_running(
        &mut conn_a,
        sync_start_a_id,
        "idle_sync_gap_inspection sync start A",
    )
    .await?;

    // Build resident encrypted rooms with a large persisted history.
    for room_index in 0..room_count {
        let room_id = create_room_for_qa(
            &mut conn_a,
            &format!("Idle Gap Inspection Room {room_index}"),
            true,
            "idle_sync_gap_inspection create room",
        )
        .await?;
        let key = TimelineKey::room(account_key_a.clone(), room_id);
        subscribe_timeline_for_qa(&mut conn_a, &key, "idle_sync_gap_inspection subscribe").await?;
        for message_index in 0..messages_per_room {
            send_text_expect_local_echo(
                &mut conn_a,
                &key,
                &format!("idle-{room_index}-{message_index}"),
                &format!("Idle gap inspection body {room_index}-{message_index}"),
                "idle_sync_gap_inspection send",
            )
            .await?;
        }
        wait_for_event_item_with_body(
            &mut conn_a,
            &key,
            &format!(
                "Idle gap inspection body {room_index}-{}",
                messages_per_room - 1
            ),
            "idle_sync_gap_inspection last body projected",
        )
        .await?;
    }
    eprintln!(
        "idle_sync_gap_inspection: resident rooms={room_count} messages_per_room={messages_per_room}"
    );

    // Idle window: the sliding-sync long polls keep producing committed
    // responses (including zero-room ones); each one schedules a LiveEdge
    // inspection per omitted resident room. Wait until at least one zero-room
    // committed response lands (the amplifier observed in the field), then
    // give its inspections a bounded settlement before harvesting.
    let baseline = koushi_diagnostics::test_support::detail_cursor();
    let mut elapsed_ms = 0u64;
    loop {
        let slice_ms = 10_000.min(idle_window_ms - elapsed_ms);
        tokio::time::sleep(Duration::from_millis(slice_ms)).await;
        elapsed_ms += slice_ms;
        let saw_zero_room = committed_response_records_since(baseline)
            .iter()
            .any(|record| {
                diagnostic_count_field(&record.event, "rooms_from_response").unwrap_or(1) == 0
            });
        if saw_zero_room {
            break;
        }
        if elapsed_ms >= idle_window_ms {
            return Err(format!(
                "idle_sync_gap_inspection: no zero-room committed response observed in the {idle_window_ms} ms window (widen KOUSHI_QA_IDLE_INSPECTION_WINDOW_MS)"
            ));
        }
    }
    tokio::time::sleep(Duration::from_millis(INSPECTION_SETTLEMENT_MS)).await;

    let inspections = gap_inspection_records_since(baseline);
    let committed = committed_response_records_since(baseline);
    let live_edge_inspections = inspections
        .iter()
        .filter(|record| diagnostic_token_field(&record.event, "trigger") == Some("live_edge"))
        .count();
    if inspections.is_empty() || live_edge_inspections == 0 {
        return Err(
            "idle_sync_gap_inspection: no completed live_edge inspection observed in the window (global-commit fan-out did not schedule one; widen KOUSHI_QA_IDLE_INSPECTION_WINDOW_MS)".to_owned(),
        );
    }

    let mut complete_durations = Vec::new();
    let mut gapped_durations = Vec::new();
    let mut scanned_chunks = 0u64;
    let mut complete_decodes = 0u64;
    let mut gapped_decodes = 0u64;
    let mut complete_count = 0usize;
    let mut gapped_count = 0usize;
    let mut unknown_count = 0usize;
    for record in &inspections {
        scanned_chunks += diagnostic_count_field(&record.event, "scanned_chunks").unwrap_or(0);
        let decodes = diagnostic_count_field(&record.event, "event_bodies_decoded").unwrap_or(0);
        let duration = diagnostic_count_field(&record.event, "duration_ms").unwrap_or(0);
        match continuity_of(record) {
            "complete" => {
                complete_count += 1;
                complete_decodes += decodes;
                complete_durations.push(duration);
            }
            "gapped" => {
                gapped_count += 1;
                gapped_decodes += decodes;
                gapped_durations.push(duration);
            }
            "unknown" => unknown_count += 1,
            continuity => {
                return Err(format!(
                    "idle_sync_gap_inspection: unexpected continuity {continuity}"
                ));
            }
        }
    }
    if complete_count == 0 {
        return Err("idle_sync_gap_inspection: no complete inspection observed".to_owned());
    }
    if complete_decodes != 0 {
        return Err(format!(
            "idle_sync_gap_inspection: complete rooms decoded {complete_decodes} event bodies; expected 0"
        ));
    }

    let (complete_p50, complete_p95) = p50_p95(complete_durations);
    let (gapped_p50, gapped_p95) = if gapped_durations.is_empty() {
        (0, 0)
    } else {
        p50_p95(gapped_durations)
    };
    let zero_room_commits = committed
        .iter()
        .filter(|record| {
            diagnostic_count_field(&record.event, "rooms_from_response").unwrap_or(1) == 0
        })
        .count();
    if zero_room_commits == 0 {
        return Err(format!(
            "idle_sync_gap_inspection: no zero-room committed response observed in the {idle_window_ms} ms window (widen KOUSHI_QA_IDLE_INSPECTION_WINDOW_MS)"
        ));
    }

    eprintln!(
        "idle_sync_gap_inspection: inspections={} complete={complete_count} gapped={gapped_count} \
         unknown={unknown_count} zero_room_commits={zero_room_commits} \
         scanned_chunks={scanned_chunks} complete_decodes={complete_decodes} \
         gapped_decodes={gapped_decodes} complete duration p50={complete_p50}ms p95={complete_p95}ms \
         gapped duration p50={gapped_p50}ms p95={gapped_p95}ms",
        inspections.len(),
    );
    println!("idle_sync_gap_inspection=ok");

    cleanup_logged_in_runtime(
        conn_a,
        runtime_a,
        account_key_a,
        "idle_sync_gap_inspection cleanup A",
    )
    .await?;
    Ok(())
}
