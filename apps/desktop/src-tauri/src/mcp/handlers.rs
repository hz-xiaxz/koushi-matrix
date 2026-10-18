//! Tool handlers. Every handler returns a JSON value; the caller wraps it in an
//! MCP tool result.
//!
//! Only account-scoped reads are implemented in this slice. Anything else
//! answers `unsupported` instead of pretending to work.

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use koushi_mcp::wire::{ErrorCode, WireError};
use koushi_state::{SessionInfo, SessionState};

use crate::CoreRuntimeState;

/// One `tools/call` request.
pub(crate) async fn dispatch_tool_call(app: &AppHandle, params: Value) -> Result<Value, WireError> {
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
        "list_accounts" => list_accounts(app),
        "list_rooms" => list_rooms(app, &arguments),
        "search_messages"
        | "get_messages"
        | "get_message_context"
        | "get_attachment"
        | "send_message"
        | "wait_for_events" => Err(WireError::new(
            ErrorCode::Unsupported,
            format!("{name} is not implemented yet"),
        )),
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

fn list_accounts(app: &AppHandle) -> Result<Value, WireError> {
    let state = app.state::<CoreRuntimeState>();
    let runtime = std::sync::Arc::clone(&state.runtime);
    let mut accounts = Vec::new();
    for tab in runtime.tab_descriptors() {
        let Some(connection) = runtime.tab_connection(&tab.id) else {
            continue;
        };
        let snapshot = connection.snapshot();
        let (status, session) = match &snapshot.session {
            SessionState::Ready(info) => ("ready", Some(info)),
            SessionState::Locked(info) => ("locked", Some(info)),
            SessionState::AwaitingVerification { info, .. } => ("verification_pending", Some(info)),
            SessionState::Provisional { info, .. } => ("provisional", Some(info)),
            SessionState::SignedOut => ("signed_out", None),
            _ => ("unavailable", None),
        };
        let user_id = session.map(|info| info.user_id.clone());
        let homeserver = session
            .map(|info| info.homeserver.clone())
            .or_else(|| tab.homeserver.clone());
        let display_name = session.map(display_name_from_user_id);
        accounts.push(json!({
            "account_id": tab.id.as_str(),
            "user_id": user_id,
            "homeserver": homeserver,
            "display_name": display_name,
            "status": status,
        }));
    }
    Ok(json!({ "accounts": accounts }))
}

fn display_name_from_user_id(info: &SessionInfo) -> String {
    info.user_id
        .strip_prefix('@')
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(&info.user_id)
        .to_owned()
}

fn list_rooms(app: &AppHandle, arguments: &Value) -> Result<Value, WireError> {
    let account_id = arguments
        .get("account_id")
        .and_then(Value::as_str)
        .ok_or_else(|| WireError::new(ErrorCode::Unsupported, "account_id is required"))?;
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

    let state = app.state::<CoreRuntimeState>();
    let runtime = std::sync::Arc::clone(&state.runtime);
    let tab_id =
        koushi_core::account_runtime_manager::AccountTabId::from_string(account_id.to_owned());
    let connection = runtime
        .tab_connection(&tab_id)
        .ok_or_else(|| WireError::new(ErrorCode::AccountUnavailable, "unknown account"))?;
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
