//! Tool handlers. Every handler returns a JSON value; the caller wraps it in an
//! MCP tool result.
//!
//! Reads require a read grant, sends require a send grant. Grants are loaded per
//! call from `mcp/connections.json`, so revoking one takes effect immediately.

use std::path::Path;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use koushi_mcp::wire::{ErrorCode, WireError};
use koushi_protocol::command::{
    CoreCommand, MediaDownloadSelection, SearchCommand, SearchScope, TimelineCommand,
};
use koushi_protocol::event::{CoreEvent, SearchEvent, TimelineEvent};
use koushi_protocol::ids::TimelineKey;
use koushi_state::{ComposerDocument, SessionInfo, SessionState};

use super::grants::{self, Need};
use crate::{CoreRuntimeState, commands};

/// One `tools/call` request.
pub(crate) async fn dispatch_tool_call(
    app: &AppHandle,
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
        "list_accounts" => list_accounts(app, client_name, data_dir),
        "list_rooms" => list_rooms(app, client_name, data_dir, &arguments),
        "search_messages" => search_messages(app, client_name, data_dir, &arguments).await,
        "send_message" => send_message(app, client_name, data_dir, &arguments).await,
        "wait_for_events" => wait_for_events(app, client_name, data_dir, &arguments).await,
        "get_attachment" => get_attachment(app, client_name, data_dir, &arguments).await,
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
fn list_accounts(app: &AppHandle, client_name: &str, data_dir: &Path) -> Result<Value, WireError> {
    let grants = grants::load(data_dir);
    let granted: Vec<&str> = grants
        .iter()
        .filter(|grant| grant.client_name == client_name)
        .flat_map(|grant| grant.accounts.iter().map(String::as_str))
        .collect();

    let state = app.state::<CoreRuntimeState>();
    let runtime = std::sync::Arc::clone(&state.runtime);
    let mut accounts = Vec::new();
    for tab in runtime.tab_descriptors() {
        if !tab.id.is_account() {
            continue;
        }
        let Some(connection) = runtime.tab_connection(&tab.id) else {
            continue;
        };
        let snapshot = connection.snapshot();
        let (status, session) = session_status(&snapshot.session);
        if granted.contains(&tab.id.as_str()) {
            let user_id = session.map(|info| info.user_id.clone());
            let homeserver = session
                .map(|info| info.homeserver.clone())
                .or_else(|| tab.homeserver.clone());
            accounts.push(json!({
                "account_id": tab.id.as_str(),
                "user_id": user_id,
                "homeserver": homeserver,
                "display_name": session.map(display_name_from_user_id),
                "status": status,
                "consent": "granted",
            }));
        } else {
            accounts.push(json!({
                "account_id": tab.id.as_str(),
                "status": status,
                "consent": "required",
            }));
        }
    }
    Ok(json!({ "accounts": accounts }))
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
    app: &AppHandle,
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

    let connection = account_connection(app, &account_id)?;
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

async fn search_messages(
    app: &AppHandle,
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

    let mut connection = account_connection(app, &account_id)?;
    let room_names: std::collections::HashMap<String, String> = connection
        .snapshot()
        .rooms
        .iter()
        .map(|room| (room.room_id.clone(), room.display_label.clone()))
        .collect();

    let request_id = connection.next_request_id();
    let room_filter = if room_ids.is_empty() {
        koushi_state::SearchRoomFilter::AllRooms
    } else {
        koushi_state::SearchRoomFilter::OnlyRooms(room_ids)
    };
    connection
        .command(CoreCommand::Search(SearchCommand::Query {
            request_id,
            query,
            scope: SearchScope::AllRooms,
            room_filter,
        }))
        .await
        .map_err(|_| WireError::new(ErrorCode::Internal, "search could not be submitted"))?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let event = tokio::time::timeout_at(deadline, connection.recv_event())
            .await
            .map_err(|_| WireError::new(ErrorCode::Internal, "search timed out"))?;
        match event {
            Ok(CoreEvent::Search(SearchEvent::Results {
                request_id: completed,
                results,
            })) if completed == request_id => {
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
                return Ok(json!({ "results": items, "total": total }));
            }
            Ok(_) => continue,
            Err(_) => {
                return Err(WireError::new(
                    ErrorCode::Internal,
                    "event stream lagged during search",
                ));
            }
        }
    }
}

async fn send_message(
    app: &AppHandle,
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

    let mut connection = account_connection(app, &account_id)?;
    let snapshot = connection.snapshot();
    let account_key = commands::account_key_from_app_state(&snapshot);
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
    app: &AppHandle,
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
    let log = app.state::<super::events::McpEventLog>();
    super::events::wait_for_events(&log.0, &account_id, cursor, timeout_s).await
}

/// Downloads one file of a message into Koushi's managed media cache and
/// returns its absolute path. Koushi chooses the destination; the caller never
/// does.
async fn get_attachment(
    app: &AppHandle,
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

    let mut connection = account_connection(app, &account_id)?;
    let snapshot = connection.snapshot();
    let account_key = commands::account_key_from_app_state(&snapshot);
    if account_key.0.is_empty() {
        return Err(WireError::new(
            ErrorCode::AccountUnavailable,
            "account is not signed in",
        ));
    }
    let key = TimelineKey::room(account_key, room_id.clone());
    let request_id = connection.next_request_id();
    let selection = if thumbnail {
        MediaDownloadSelection::Thumbnail {
            width: 800,
            height: 800,
        }
    } else {
        MediaDownloadSelection::File
    };
    connection
        .command(CoreCommand::Timeline(TimelineCommand::DownloadMedia {
            request_id,
            key: key.clone(),
            event_id: event_id.clone(),
            selection,
        }))
        .await
        .map_err(|_| WireError::new(ErrorCode::Internal, "download could not be submitted"))?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let event = tokio::time::timeout_at(deadline, connection.recv_event())
            .await
            .map_err(|_| WireError::new(ErrorCode::Internal, "download timed out"))?;
        match event {
            Ok(CoreEvent::Timeline(TimelineEvent::MediaDownloadCompleted {
                request_id: completed,
                event_id: completed_event,
                source_url,
                byte_count,
                mimetype,
                ..
            })) if completed == request_id && completed_event == event_id => {
                let path =
                    resolve_download_path(app, &account_id, &source_url).ok_or_else(|| {
                        WireError::new(
                            ErrorCode::Internal,
                            "download finished without a path inside Koushi's media cache",
                        )
                    })?;
                return Ok(json!({
                    "room_id": room_id,
                    "event_id": event_id,
                    "path": path.to_string_lossy(),
                    "mime_type": mimetype,
                    "size_bytes": byte_count,
                }));
            }
            Ok(CoreEvent::Timeline(TimelineEvent::MediaDownloadFailed {
                request_id: completed,
                ..
            })) if completed == request_id => {
                return Err(WireError::new(
                    ErrorCode::Internal,
                    "Koushi could not download this attachment",
                ));
            }
            Ok(_) => continue,
            Err(_) => {
                return Err(WireError::new(
                    ErrorCode::Internal,
                    "event stream lagged during download",
                ));
            }
        }
    }
}

/// Maps a downloaded asset URL onto the per-account media cache, refusing any
/// path that leaves it.
fn resolve_download_path(
    app: &AppHandle,
    account_id: &str,
    source_url: &str,
) -> Option<std::path::PathBuf> {
    let state = app.state::<CoreRuntimeState>();
    let tab_id =
        koushi_core::account_runtime_manager::AccountTabId::from_string(account_id.to_owned());
    let cache = state.runtime.media_cache_dir_for_tab(&tab_id)?;
    let relative = source_url.split("media-downloads/").nth(1)?;
    let canonical_cache = cache.canonicalize().ok()?;
    let candidate = cache.join(relative).canonicalize().ok()?;
    candidate.starts_with(&canonical_cache).then_some(candidate)
}

fn account_connection(
    app: &AppHandle,
    account_id: &str,
) -> Result<koushi_core::CoreConnection, WireError> {
    let state = app.state::<CoreRuntimeState>();
    let runtime = std::sync::Arc::clone(&state.runtime);
    let tab_id =
        koushi_core::account_runtime_manager::AccountTabId::from_string(account_id.to_owned());
    runtime
        .tab_connection(&tab_id)
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
