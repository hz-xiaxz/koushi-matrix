//! Commands for the AI assistants settings page and approval dialog (#1536).
//! The consent broker owns every decision; these only forward the user's
//! answers and return closed outcome codes.

use koushi_mcp_host::Need;
use koushi_mcp_host::consent::{AnswerOutcome, McpAssistantsState};
use tauri::State;

use crate::mcp::McpController;

/// The current state, after re-reading the grant store so a hand edit shows.
#[tauri::command]
pub fn get_mcp_assistants_state(state: State<'_, McpController>) -> McpAssistantsState {
    state.broker.refresh()
}

/// Answers one approval. Returns `applied`, `stale` (already answered or
/// expired), or `store_failed` (the grant could not be saved).
#[tauri::command]
pub async fn answer_mcp_approval(
    state: State<'_, McpController>,
    request_id: u64,
    allow: bool,
) -> Result<&'static str, ()> {
    Ok(match state.broker.answer(request_id, allow).await {
        AnswerOutcome::Applied => "applied",
        AnswerOutcome::Stale => "stale",
        AnswerOutcome::StoreFailed => "store_failed",
    })
}

/// Turns one permission on or off for one assistant and account. A failure
/// is reported through the state's `store` status.
#[tauri::command]
pub async fn set_mcp_permission(
    state: State<'_, McpController>,
    client_name: String,
    account_id: String,
    need: Need,
    allowed: bool,
) -> Result<(), ()> {
    state
        .broker
        .set_permission(&client_name, &account_id, need, allowed)
        .await
        .map_err(|_| ())
}

/// Removes an assistant's grants, open approvals, and remembered denials.
#[tauri::command]
pub async fn remove_mcp_assistant(
    state: State<'_, McpController>,
    client_name: String,
) -> Result<(), ()> {
    state
        .broker
        .remove_client(&client_name)
        .await
        .map_err(|_| ())
}
