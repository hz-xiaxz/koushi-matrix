use super::*;
use koushi_diagnostics::{record, DiagnosticEvent, DiagnosticField, DiagnosticLevel};
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

fn resolve_search_scope_from_active_room(
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

fn resolve_search_scope_from_active_room(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::contracts::fake_request_id;

    #[test]
    fn search_scope_resolution_preserves_non_all_scope_contract() {
        let source = commands_source();
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("command production source should precede tests");
        let resolver = production_source
            .split("fn resolve_search_scope_from_active_room")
            .nth(1)
            .expect("search scope resolver should exist")
            .split("async fn resolve_search_scope")
            .next()
            .expect("async search scope resolver should follow pure resolver");

        assert!(
                resolver.contains("SearchScope::CurrentSpace"),
                "current-space searches must preserve the selected scope kind instead of collapsing to global"
            );
        assert!(
                resolver.contains("SearchScope::CurrentRoom"),
                "Room/DM searches must preserve the selected conversation instead of collapsing to global"
            );
        assert!(
            !resolver.contains("unwrap_or(SearchScope::AllRooms)"),
            "non-all search scopes must not silently round-trip as allRooms"
        );
    }

    #[test]
    fn submit_search_returns_after_correlated_search_start_before_result_completion() {
        let source = commands_source();
        let search_source = include_str!("search.rs");
        let fn_name = "pub async fn submit_search";

        let fn_offset = source
            .find(fn_name)
            .expect("submit_search command should exist");
        let rest = &source[fn_offset..];
        let end = rest
            .find("pub async fn start_room_crawl")
            .expect("start_room_crawl command should follow submit_search");
        let command_source = &rest[..end];

        let helper_offset = search_source
            .find("pub(crate) async fn submit_search_production_path")
            .expect("submit_search should use the shared production path");
        let helper_source = &search_source[helper_offset..];
        let attach_offset = helper_source
            .find("let mut event_conn = state.runtime.attach()")
            .expect("production search path should attach a transient event listener");
        let request_offset = helper_source
            .find("let request_id = next_request_id(state).await")
            .expect("production search path should allocate request ids");
        let submit_offset = helper_source
            .find("io.submit")
            .expect("production search path should submit through its internal port");
        let wait_offset = helper_source
            .find("io.wait")
            .expect("production search path should wait for correlated search start");
        let snapshot_offset = command_source
            .find("current_snapshot")
            .expect("submit_search should return a snapshot");
        assert!(
            attach_offset < request_offset
                && request_offset < submit_offset
                && submit_offset < wait_offset,
            "production search path should return after correlated search start"
        );
        let call_offset = command_source
            .find("submit_search_production_path")
            .expect("submit_search should call the shared production path");
        assert!(
            call_offset < snapshot_offset,
            "submit_search should return the searching snapshot after the production path"
        );
        assert!(
                !helper_source.contains("let request_id = event_conn.next_request_id()"),
                "submit_search must not use transient event-connection sequence numbers for state correlation"
            );
        assert!(
            !helper_source.contains("wait_for_search_completed"),
            "submit_search must not block the renderer on search result completion"
        );
    }
}
