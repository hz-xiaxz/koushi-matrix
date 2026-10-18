//! Tool handlers. Every handler returns a JSON value; the caller wraps it in an
//! MCP tool result.
//!
//! Reads require a read grant, sends require a send grant. Grants are loaded per
//! call from `mcp/connections.json`, so revoking one takes effect immediately.

use std::path::Path;

use serde_json::{Value, json};
use std::sync::Arc;

use crate::BridgeAccounts;

use koushi_mcp::wire::{ErrorCode, WireError};
use koushi_protocol::command::{CoreCommand, TimelineCommand};
use koushi_protocol::event::{CoreEvent, TimelineEvent};
use koushi_protocol::ids::TimelineKey;
use koushi_state::{ComposerDocument, SessionInfo, SessionState};

use super::events;
use super::grants::{self, Need};

/// One `tools/call` request.
pub(crate) async fn dispatch_tool_call(
    accounts: &Arc<dyn BridgeAccounts>,
    log: &Arc<events::EventLog>,
    client_name: &str,
    data_dir: &Path,
    params: Value,
) -> Result<Value, WireError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(WireError::new(
            ErrorCode::Unsupported,
            "tools/call requires a tool name",
        ));
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result = match name {
        "list_accounts" => list_accounts(accounts, client_name, data_dir),
        "list_rooms" => list_rooms(accounts, client_name, data_dir, &arguments),
        "search_messages" => search_messages(accounts, client_name, data_dir, &arguments).await,
        "send_message" => send_message(accounts, client_name, data_dir, &arguments).await,
        "wait_for_events" => {
            wait_for_events(accounts, log, client_name, data_dir, &arguments).await
        }
        "get_attachment" => get_attachment(accounts, client_name, data_dir, &arguments).await,
        other => Err(WireError::new(
            ErrorCode::Unsupported,
            format!("unknown tool: {other}"),
        )),
    };

    match result {
        Ok(value) => Ok(json!({
            "content": [{ "type": "text", "text": value.to_string() }],
            "isError": false
        })),
        Err(error) => Ok(json!({
            "content": [{ "type": "text", "text": format!("{}: {}", error.code.as_str(), error.message) }],
            "isError": true
        })),
    }
}

/// Every account is listed so a first-time client can be granted access, but
/// identity fields appear only once a grant exists.
fn list_accounts(
    accounts: &Arc<dyn BridgeAccounts>,
    client_name: &str,
    data_dir: &Path,
) -> Result<Value, WireError> {
    let grants = grants::load(data_dir);
    let granted: Vec<&str> = grants
        .iter()
        .filter(|grant| grant.client_name == client_name)
        .flat_map(|grant| grant.accounts.iter().map(String::as_str))
        .collect();
    let mut listed = Vec::new();
    for account in accounts.accounts() {
        let snapshot = account.connection.snapshot();
        let (status, session) = session_status(&snapshot.session);
        if granted.contains(&account.id.as_str()) {
            let user_id = session.map(|info| info.user_id.clone());
            let homeserver = session
                .map(|info| info.homeserver.clone())
                .or_else(|| account.homeserver.clone());
            listed.push(json!({
                "account_id": account.id,
                "user_id": user_id,
                "homeserver": homeserver,
                "display_name": session.map(display_name_from_user_id),
                "status": status,
                "consent": "granted",
            }));
        } else {
            listed.push(json!({
                "account_id": account.id,
                "status": status,
                "consent": "required",
            }));
        }
    }
    Ok(json!({ "accounts": listed }))
}

fn session_status(session: &SessionState) -> (&'static str, Option<&SessionInfo>) {
    match session {
        SessionState::Ready(info) => ("ready", Some(info)),
        SessionState::Locked(info) => ("locked", Some(info)),
        SessionState::AwaitingVerification { info, .. } => ("verification_pending", Some(info)),
        SessionState::Provisional { info, .. } => ("provisional", Some(info)),
        SessionState::SignedOut => ("signed_out", None),
        _ => ("unavailable", None),
    }
}

fn display_name_from_user_id(info: &SessionInfo) -> String {
    info.user_id
        .strip_prefix('@')
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(&info.user_id)
        .to_owned()
}

fn list_rooms(
    accounts: &Arc<dyn BridgeAccounts>,
    client_name: &str,
    data_dir: &Path,
    arguments: &Value,
) -> Result<Value, WireError> {
    let account_id = required_account_id(arguments)?;
    grants::authorize(
        &grants::load(data_dir),
        client_name,
        &account_id,
        Need::Read,
    )?;
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;
    let offset = arguments
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);

    let connection = account_connection(accounts, &account_id)?;
    let snapshot = connection.snapshot();
    let mut rooms: Vec<Value> = snapshot
        .rooms
        .iter()
        .map(|room| {
            json!({
                "room_id": room.room_id,
                "name": room.display_label,
                "is_dm": room.is_dm,
                "is_encrypted": room.is_encrypted,
            })
        })
        .collect();

    let total = rooms.len();
    let page: Vec<Value> = rooms.drain(..).skip(offset).take(limit).collect();
    let next = offset + page.len();
    Ok(json!({
        "rooms": page,
        "next_cursor": (next < total).then(|| next.to_string()),
    }))
}

fn direct_search_failure(failure: koushi_core::search::DirectSearchFailure) -> WireError {
    use koushi_core::search::DirectSearchFailure as Failure;
    let (code, message) = match failure {
        Failure::AccountInactive => (ErrorCode::AccountUnavailable, "account is not signed in"),
        Failure::Capacity => (
            ErrorCode::Busy,
            "too many concurrent searches for this account",
        ),
        Failure::ForbiddenRoom => (ErrorCode::RoomNotFound, "a requested room is not joined"),
        Failure::Search(koushi_protocol::failure::SearchFailureKind::IndexUnavailable) => {
            (ErrorCode::Internal, "the search index is unavailable")
        }
        Failure::Search(_) => (ErrorCode::Internal, "search failed"),
    };
    WireError::new(code, message)
}

async fn search_messages(
    accounts: &Arc<dyn BridgeAccounts>,
    client_name: &str,
    data_dir: &Path,
    arguments: &Value,
) -> Result<Value, WireError> {
    let account_id = required_account_id(arguments)?;
    grants::authorize(
        &grants::load(data_dir),
        client_name,
        &account_id,
        Need::Read,
    )?;
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "query is required"))?
        .trim()
        .to_owned();
    if query.is_empty() {
        return Err(WireError::new(ErrorCode::Unsupported, "query is empty"));
    }
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;
    let room_ids: Vec<String> = arguments
        .get("room_ids")
        .and_then(Value::as_array)
        .map(|rooms| {
            rooms
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let connection = account_connection(accounts, &account_id)?;
    let room_names: std::collections::HashMap<String, String> = connection
        .snapshot()
        .rooms
        .iter()
        .map(|room| (room.room_id.clone(), room.display_label.clone()))
        .collect();

    let results = connection
        .direct_search(query, room_ids)
        .await
        .map_err(direct_search_failure)?;

    let total = results.len();
    let items: Vec<Value> = results
        .into_iter()
        .take(limit)
        .map(|item| {
            let room_name = room_names
                .get(&item.room_id)
                .cloned()
                .unwrap_or_else(|| item.room_id.clone());
            json!({
                "room_id": item.room_id,
                "room_name": room_name,
                "event_id": item.event_id,
                "snippet": item.snippet,
            })
        })
        .collect();
    Ok(json!({ "results": items, "total": total }))
}

async fn send_message(
    accounts: &Arc<dyn BridgeAccounts>,
    client_name: &str,
    data_dir: &Path,
    arguments: &Value,
) -> Result<Value, WireError> {
    let account_id = required_account_id(arguments)?;
    grants::authorize(
        &grants::load(data_dir),
        client_name,
        &account_id,
        Need::Send,
    )?;
    let room_id = arguments
        .get("room_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "room_id is required"))?
        .to_owned();
    let body = arguments
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if body.trim().is_empty() {
        return Err(WireError::new(ErrorCode::Unsupported, "body is empty"));
    }
    let in_reply_to = arguments
        .get("reply_to_event_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let client_request_id = arguments
        .get("client_request_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());

    let mut connection = account_connection(accounts, &account_id)?;
    let snapshot = connection.snapshot();
    let account_key = account_key_from_snapshot(&snapshot);
    if account_key.0.is_empty() {
        return Err(WireError::new(
            ErrorCode::AccountUnavailable,
            "account is not signed in",
        ));
    }
    let key = TimelineKey::room(account_key, room_id.clone());
    let request_id = connection.next_request_id();
    // Caller-supplied ids make a retried send idempotent at the core.
    let transaction_id = client_request_id
        .map(|id| format!("mcp-{id}"))
        .unwrap_or_else(|| format!("mcp-{}", request_id.sequence));
    let document = ComposerDocument::from_plain_text(body);
    let command = match in_reply_to {
        Some(event_id) => TimelineCommand::SendReply {
            request_id,
            key: key.clone(),
            transaction_id: transaction_id.clone(),
            in_reply_to_event_id: event_id,
            document,
        },
        None => TimelineCommand::SendText {
            request_id,
            key: key.clone(),
            transaction_id: transaction_id.clone(),
            document,
        },
    };
    connection
        .command(CoreCommand::Timeline(command))
        .await
        .map_err(|_| WireError::new(ErrorCode::Internal, "send could not be submitted"))?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let event = tokio::time::timeout_at(deadline, connection.recv_event())
            .await
            .map_err(|_| WireError::new(ErrorCode::Internal, "send timed out"))?;
        match event {
            Ok(CoreEvent::Timeline(TimelineEvent::SendCompleted {
                request_id: completed,
                key: completed_key,
                transaction_id,
                event_id,
            })) if completed == request_id && completed_key == key => {
                return Ok(json!({
                    "room_id": room_id,
                    "event_id": event_id,
                    "transaction_id": transaction_id,
                }));
            }
            Ok(CoreEvent::OperationFailed {
                request_id: completed,
                failure,
            }) if completed == request_id => {
                return Err(WireError::new(
                    ErrorCode::Internal,
                    format!("send failed: {failure:?}"),
                ));
            }
            Ok(_) => continue,
            Err(_) => {
                return Err(WireError::new(
                    ErrorCode::Internal,
                    "event stream lagged during send",
                ));
            }
        }
    }
}

async fn wait_for_events(
    _accounts: &Arc<dyn BridgeAccounts>,
    log: &Arc<events::EventLog>,
    client_name: &str,
    data_dir: &Path,
    arguments: &Value,
) -> Result<Value, WireError> {
    let account_id = required_account_id(arguments)?;
    grants::authorize(
        &grants::load(data_dir),
        client_name,
        &account_id,
        Need::Read,
    )?;
    let cursor = arguments.get("cursor").and_then(Value::as_str);
    let timeout_s = arguments
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(25);

    events::wait_for_events(log, &account_id, cursor, timeout_s).await
}

/// Downloads one file of a message into Koushi's managed media cache and
/// returns its absolute path. Koushi chooses the destination; the caller never
/// does.
async fn get_attachment(
    accounts: &Arc<dyn BridgeAccounts>,
    client_name: &str,
    data_dir: &Path,
    arguments: &Value,
) -> Result<Value, WireError> {
    let account_id = required_account_id(arguments)?;
    grants::authorize(
        &grants::load(data_dir),
        client_name,
        &account_id,
        Need::Attachments,
    )?;
    let room_id = arguments
        .get("room_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "room_id is required"))?
        .to_owned();
    let event_id = arguments
        .get("event_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "event_id is required"))?
        .to_owned();
    let thumbnail = arguments
        .get("thumbnail")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let connection = account_connection(accounts, &account_id)?;
    let attachment = connection
        .fetch_attachment(room_id.clone(), event_id.clone(), thumbnail)
        .await
        .map_err(attachment_failure)?;
    ensure_cache_containment(accounts, &account_id, &attachment.path)?;
    Ok(json!({
        "room_id": room_id,
        "event_id": event_id,
        "path": attachment.path.to_string_lossy(),
        "mime_type": attachment.mime_type,
        "size_bytes": attachment.size_bytes,
    }))
}

fn attachment_failure(failure: koushi_core::AttachmentFetchFailure) -> WireError {
    use koushi_core::AttachmentFetchFailure as Failure;
    let (code, message) = match failure {
        Failure::AccountInactive => (ErrorCode::AccountUnavailable, "account is not signed in"),
        Failure::RoomNotFound => (ErrorCode::RoomNotFound, "the room is not joined"),
        Failure::EventNotFound => (ErrorCode::EventNotFound, "the event is unknown to the room"),
        Failure::Unsupported => (ErrorCode::Unsupported, "the event carries no attachment"),
        Failure::DownloadFailed => (
            ErrorCode::Internal,
            "Koushi could not download this attachment",
        ),
    };
    WireError::new(code, message)
}

/// Refuse any returned path that leaves the account's media cache.
fn ensure_cache_containment(
    accounts: &Arc<dyn BridgeAccounts>,
    account_id: &str,
    path: &std::path::Path,
) -> Result<(), WireError> {
    let cache = accounts
        .accounts()
        .into_iter()
        .find(|account| account.id == account_id)
        .map(|account| account.media_cache_dir)
        .ok_or_else(|| WireError::new(ErrorCode::Internal, "unknown account cache"))?;
    let canonical_cache = cache
        .canonicalize()
        .map_err(|_| WireError::new(ErrorCode::Internal, "unknown account cache"))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| WireError::new(ErrorCode::Internal, "attachment path is unavailable"))?;
    if canonical_path.starts_with(&canonical_cache) {
        Ok(())
    } else {
        Err(WireError::new(
            ErrorCode::Internal,
            "download finished outside Koushi's media cache",
        ))
    }
}

fn account_connection(
    accounts: &Arc<dyn BridgeAccounts>,
    account_id: &str,
) -> Result<koushi_core::CoreConnection, WireError> {
    accounts
        .accounts()
        .into_iter()
        .find(|account| account.id == account_id)
        .map(|account| account.connection)
        .ok_or_else(|| WireError::new(ErrorCode::AccountUnavailable, "unknown account"))
}

fn required_account_id(arguments: &Value) -> Result<String, WireError> {
    arguments
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "account_id is required"))
}

/// The signed-in account key, or the empty key when no session is ready.
pub(crate) fn account_key_from_snapshot(
    snapshot: &koushi_state::AppState,
) -> koushi_protocol::ids::AccountKey {
    match &snapshot.session {
        koushi_state::SessionState::Ready(info)
        | koushi_state::SessionState::Provisional { info, .. }
        | koushi_state::SessionState::AwaitingVerification { info, .. }
        | koushi_state::SessionState::Verifying { info, .. }
        | koushi_state::SessionState::AwaitingBootstrapConfirmation { info, .. }
        | koushi_state::SessionState::Rejecting { info, .. }
        | koushi_state::SessionState::Locked(info) => {
            koushi_core::store::account_key_from_info(info)
        }
        _ => koushi_protocol::ids::AccountKey(String::new()),
    }
}
