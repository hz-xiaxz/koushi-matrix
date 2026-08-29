use super::*;
use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record};
use std::{future::Future, pin::Pin};

fn search_scope_kind_trace_label(scope: SearchScopeKind) -> &'static str {
    match scope {
        SearchScopeKind::CurrentRoom => "current_room",
        SearchScopeKind::CurrentSpace => "current_space",
        SearchScopeKind::AllRooms => "all_rooms",
    }
}

fn resolved_search_scope_trace_label(scope: &SearchScope) -> &'static str {
    match scope {
        SearchScope::CurrentRoom { .. } => "current_room",
        SearchScope::CurrentSpace { .. } => "current_space",
        SearchScope::AllRooms => "all_rooms",
    }
}

pub(crate) fn record_search_trace(
    scope: SearchScopeKind,
    search_scope: &SearchScope,
    query: &str,
    request_id: koushi_core::RequestId,
) {
    let trimmed_query = query.trim();
    record(
        DiagnosticEvent::new(DiagnosticLevel::Debug, "desktop.search", "submit")
            .field(DiagnosticField::token(
                "ui_scope",
                search_scope_kind_trace_label(scope),
            ))
            .field(DiagnosticField::token(
                "resolved_scope",
                resolved_search_scope_trace_label(search_scope),
            ))
            .field(DiagnosticField::count(
                "query_bytes",
                trimmed_query.len() as u64,
            ))
            .field(DiagnosticField::count(
                "query_chars",
                trimmed_query.chars().count() as u64,
            ))
            .field(DiagnosticField::request_id(
                "request_id",
                request_id.connection_id.0,
                request_id.sequence,
            )),
    );
}

#[tauri::command]
pub async fn submit_search(
    query: String,
    scope: SearchScopeKind,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let search_scope = resolve_search_scope(scope, state.inner()).await;
    submit_search_production_path(
        query,
        scope,
        search_scope,
        state.inner(),
        &ProductionSearchPathIo,
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

/// Command-body boundary used by `submit_search` and its Tauri-adapter test.
/// Keeping the runtime submission and correlated wait here exercises the same
/// production path without requiring a platform-specific `AppHandle` in the
/// mock-runtime child.
pub(crate) async fn submit_search_production_path(
    query: String,
    scope: SearchScopeKind,
    search_scope: SearchScope,
    state: &CoreRuntimeState,
    io: &impl SearchPathIo,
) -> Result<(), String> {
    let mut event_conn = state.runtime.attach();
    let request_id = next_request_id(state).await;
    record_search_trace(scope, &search_scope, &query, request_id);
    io.submit(
        state,
        build_submit_search_command(request_id, query, search_scope),
    )
    .await?;
    io.wait(&mut event_conn, request_id).await?;
    Ok(())
}

pub(crate) type SearchPathFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub(crate) trait SearchPathIo {
    fn submit<'a>(
        &'a self,
        state: &'a CoreRuntimeState,
        command: CoreCommand,
    ) -> SearchPathFuture<'a>;
    fn wait<'a>(
        &'a self,
        connection: &'a mut CoreConnection,
        request_id: RequestId,
    ) -> SearchPathFuture<'a>;
}

struct ProductionSearchPathIo;

impl SearchPathIo for ProductionSearchPathIo {
    fn submit<'a>(
        &'a self,
        state: &'a CoreRuntimeState,
        command: CoreCommand,
    ) -> SearchPathFuture<'a> {
        Box::pin(async move { submit_core_command(state, command).await })
    }

    fn wait<'a>(
        &'a self,
        connection: &'a mut CoreConnection,
        request_id: RequestId,
    ) -> SearchPathFuture<'a> {
        Box::pin(async move {
            wait_for_search_started(connection, request_id, SEARCH_EVENT_TIMEOUT).await
        })
    }
}

#[tauri::command]
pub async fn close_search(
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let mut event_conn = state.runtime.attach();
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(state.inner(), build_close_search_command(request_id)).await?;
    wait_for_search_closed(&mut event_conn, request_id, SEARCH_EVENT_TIMEOUT).await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn start_room_crawl(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    // Read current crawler settings from the Rust-owned snapshot so this
    // command doesn't duplicate settings state in the TypeScript layer.
    let settings = state
        .connection
        .lock()
        .await
        .snapshot()
        .settings
        .values
        .search_crawler
        .clone();
    submit_core_command(
        state.inner(),
        CoreCommand::Search(SearchCommand::StartHistoryCrawl {
            request_id,
            room_id,
            settings,
        }),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

#[tauri::command]
pub async fn stop_room_crawl(
    room_id: String,
    app: AppHandle,
    state: State<'_, CoreRuntimeState>,
) -> Result<FrontendDesktopSnapshot, String> {
    let request_id = next_request_id(state.inner()).await;
    submit_core_command(
        state.inner(),
        CoreCommand::Search(SearchCommand::StopHistoryCrawl {
            request_id,
            room_id,
        }),
    )
    .await?;
    update_qa_window_title_from_state(&app, state.inner()).await;
    current_snapshot(state.inner()).await
}

const SEARCH_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) async fn wait_for_search_started(
    event_conn: &mut CoreConnection,
    request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_started_search(&event_conn.snapshot(), request_id) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "search did not start".to_owned())?;
        match event {
            Ok(CoreEvent::Search(SearchEvent::Results {
                request_id: result_request_id,
                ..
            })) if result_request_id == request_id => {}
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure("search failed", failure));
            }
            Ok(CoreEvent::StateChanged(snapshot))
                if snapshot_has_started_search(&snapshot, request_id) =>
            {
                return Ok(());
            }
            Ok(_) => {}
            Err(_) if snapshot_has_started_search(&event_conn.snapshot(), request_id) => {
                return Ok(());
            }
            Err(_) => continue,
        }
    }
}

pub(super) async fn wait_for_search_closed(
    event_conn: &mut CoreConnection,
    request_id: RequestId,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if snapshot_has_closed_search(&event_conn.snapshot()) {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, event_conn.recv_event())
            .await
            .map_err(|_| "search did not close".to_owned())?;
        match event {
            Ok(CoreEvent::StateChanged(snapshot)) if snapshot_has_closed_search(&snapshot) => {
                return Ok(());
            }
            Ok(CoreEvent::OperationFailed {
                request_id: failed_request_id,
                failure,
            }) if failed_request_id == request_id => {
                return Err(invoke_error_from_core_failure(
                    "search close failed",
                    failure,
                ));
            }
            Ok(_) => {}
            Err(_) if snapshot_has_closed_search(&event_conn.snapshot()) => return Ok(()),
            Err(_) => continue,
        }
    }
}

pub(super) fn build_submit_search_command(
    request_id: koushi_core::RequestId,
    query: String,
    scope: SearchScope,
) -> CoreCommand {
    CoreCommand::Search(SearchCommand::Query {
        request_id,
        query,
        scope,
        room_filter: koushi_state::SearchRoomFilter::AllRooms,
    })
}

pub(super) fn build_close_search_command(request_id: koushi_core::RequestId) -> CoreCommand {
    CoreCommand::App(AppCommand::CloseSearch { request_id })
}

pub(super) fn resolve_search_scope_from_active_room(
    scope: SearchScopeKind,
    active_room_id: Option<String>,
    active_space_id: Option<String>,
) -> SearchScope {
    match scope {
        SearchScopeKind::CurrentRoom => active_room_id
            .map(|room_id| SearchScope::CurrentRoom { room_id })
            .unwrap_or_else(|| SearchScope::CurrentRoom {
                room_id: String::new(),
            }),
        SearchScopeKind::CurrentSpace => active_space_id
            .map(|space_id| SearchScope::CurrentSpace { space_id })
            .unwrap_or_else(|| SearchScope::CurrentSpace {
                space_id: String::new(),
            }),
        SearchScopeKind::AllRooms => SearchScope::AllRooms,
    }
}

fn snapshot_has_started_search(snapshot: &koushi_state::AppState, request_id: RequestId) -> bool {
    match &snapshot.search {
        koushi_state::SearchState::Searching {
            request_id: state_request_id,
            ..
        }
        | koushi_state::SearchState::Results {
            request_id: state_request_id,
            ..
        }
        | koushi_state::SearchState::TooShort {
            request_id: state_request_id,
            ..
        }
        | koushi_state::SearchState::Failed {
            request_id: state_request_id,
            ..
        } => *state_request_id == request_id.sequence,
        _ => false,
    }
}

fn snapshot_has_closed_search(snapshot: &koushi_state::AppState) -> bool {
    snapshot.search == koushi_state::SearchState::Closed
}

async fn resolve_search_scope(
    scope: SearchScopeKind,
    state: &CoreRuntimeState,
) -> koushi_core::SearchScope {
    let snapshot = state.connection.lock().await.snapshot();
    resolve_search_scope_from_active_room(
        scope,
        snapshot.navigation.active_room_id,
        snapshot.navigation.active_space_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
}
