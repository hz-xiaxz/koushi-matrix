# Issue #753 Rust test-structure inventory

Status: baseline captured before migration at origin/main 276d7d07.

## Counts

- First-party Rust files: 360
- include_str! invocations: 365
- Rust-source targets: 361
- Allowed non-Rust targets: 4
- Inline directly cfg(test) modules: 150
- Inline modules at or above 200 physical lines: 78
- External/path cfg(test) modules: 30
- Nested cfg(test) modules: 0
- Baseline workspace test identities: 2564
- Baseline Core QA binary test identities: 135

The four allowed non-Rust invocations are state-machine.md, windows-overlay.json, and coreEvents.generated.json twice. Structural source-rule mapping is appended during migration; completion requires zero Rust-source include_str! invocations.

## Large inline module baseline

- `apps/desktop/src-tauri/src/commands/diagnostics.rs:497` — `snapshot_tests`, 209 lines
- `apps/desktop/src-tauri/src/commands/e2ee.rs:588` — `tests`, 250 lines
- `apps/desktop/src-tauri/src/commands/native_attention.rs:387` — `tests`, 295 lines
- `apps/desktop/src-tauri/src/commands/navigation.rs:652` — `tests`, 342 lines
- `apps/desktop/src-tauri/src/commands/room.rs:1475` — `tests`, 390 lines
- `apps/desktop/src-tauri/src/commands/timeline.rs:3336` — `submission_settlement_tests`, 410 lines
- `apps/desktop/src-tauri/src/commands/timeline.rs:3791` — `issue551_moved_tests`, 434 lines
- `apps/desktop/src-tauri/src/core_event_forwarder.rs:270` — `tests`, 1932 lines
- `apps/desktop/src-tauri/src/dto.rs:942` — `tests`, 1623 lines
- `apps/desktop/src-tauri/src/lib.rs:937` — `tests`, 456 lines
- `apps/desktop/src-tauri/src/window_state.rs:534` — `tests`, 434 lines
- `crates/koushi-core/src/account_work.rs:607` — `tests`, 303 lines
- `crates/koushi-core/src/account/account_management.rs:367` — `tests`, 221 lines
- `crates/koushi-core/src/account/local_data_cleanup.rs:519` — `tests`, 562 lines
- `crates/koushi-core/src/account/recovery_backup.rs:1560` — `tests`, 840 lines
- `crates/koushi-core/src/account/routing.rs:700` — `tests`, 235 lines
- `crates/koushi-core/src/account/runtime_children.rs:514` — `tests`, 263 lines
- `crates/koushi-core/src/account/scheduled_send.rs:447` — `tests`, 503 lines
- `crates/koushi-core/src/account/session_lifecycle.rs:2153` — `tests`, 2595 lines
- `crates/koushi-core/src/account/sliding_sync.rs:691` — `tests`, 324 lines
- `crates/koushi-core/src/account/trust_gate.rs:1284` — `tests`, 1277 lines
- `crates/koushi-core/src/account/verification.rs:1811` — `tests`, 932 lines
- `crates/koushi-core/src/command/app.rs:705` — `tests`, 394 lines
- `crates/koushi-core/src/command/room.rs:624` — `tests`, 215 lines
- `crates/koushi-core/src/command/timeline.rs:739` — `tests`, 267 lines
- `crates/koushi-core/src/event/timeline.rs:1372` — `tests`, 1017 lines
- `crates/koushi-core/src/link_preview.rs:446` — `tests`, 412 lines
- `crates/koushi-core/src/live_tail_freshness.rs:501` — `tests`, 330 lines
- `crates/koushi-core/src/media_preparation.rs:1079` — `tests`, 502 lines
- `crates/koushi-core/src/mention_candidates.rs:120` — `tests`, 480 lines
- `crates/koushi-core/src/read_state.rs:1228` — `tests`, 717 lines
- `crates/koushi-core/src/renderable_thumbnail.rs:374` — `tests`, 253 lines
- `crates/koushi-core/src/room/actor.rs:1210` — `tests`, 324 lines
- `crates/koushi-core/src/room/encryption_debug.rs:927` — `tests`, 275 lines
- `crates/koushi-core/src/room/list_observer.rs:1833` — `tests`, 1125 lines
- `crates/koushi-core/src/room/normalization.rs:240` — `tests`, 546 lines
- `crates/koushi-core/src/room/operations.rs:1229` — `tests`, 357 lines
- `crates/koushi-core/src/room/space_members.rs:1084` — `tests`, 418 lines
- `crates/koushi-core/src/runtime.rs:4490` — `tests`, 2762 lines
- `crates/koushi-core/src/runtime/activity.rs:1111` — `tests`, 757 lines
- `crates/koushi-core/src/runtime/connection.rs:448` — `tests`, 576 lines
- `crates/koushi-core/src/runtime/navigation.rs:298` — `tests`, 296 lines
- `crates/koushi-core/src/runtime/reducer_support.rs:288` — `tests`, 274 lines
- `crates/koushi-core/src/search_crawler.rs:466` — `tests`, 461 lines
- `crates/koushi-core/src/search.rs:1614` — `tests`, 871 lines
- `crates/koushi-core/src/store/composer_drafts.rs:405` — `tests`, 307 lines
- `crates/koushi-core/src/store/composer_drafts.rs:930` — `store_tests`, 462 lines
- `crates/koushi-core/src/store/credential_backend.rs:987` — `tests`, 672 lines
- `crates/koushi-core/src/store/navigation.rs:142` — `tests`, 233 lines
- `crates/koushi-core/src/store/read_state.rs:298` — `tests`, 382 lines
- `crates/koushi-core/src/sync.rs:1692` — `tests`, 379 lines
- `crates/koushi-core/src/threads_list.rs:1731` — `tests`, 1448 lines
- `crates/koushi-core/src/timeline/actor.rs:2667` — `tests`, 302 lines
- `crates/koushi-core/src/timeline/composer.rs:181` — `tests`, 335 lines
- `crates/koushi-core/src/timeline/diagnostics.rs:2161` — `tests`, 1048 lines
- `crates/koushi-core/src/timeline/display_projection.rs:1564` — `tests`, 1284 lines
- `crates/koushi-core/src/timeline/gap_repair.rs:2928` — `tests`, 2424 lines
- `crates/koushi-core/src/timeline/item_projection.rs:4067` — `tests`, 1513 lines
- `crates/koushi-core/src/timeline/manager.rs:2051` — `tests`, 314 lines
- `crates/koushi-core/src/timeline/media.rs:762` — `tests`, 275 lines
- `crates/koushi-core/src/timeline/navigation.rs:2240` — `tests`, 2963 lines
- `crates/koushi-core/src/timeline/outbound_send.rs:2789` — `tests`, 3131 lines
- `crates/koushi-core/src/timeline/read_state.rs:2192` — `tests`, 3061 lines
- `crates/koushi-core/src/timeline/relay.rs:1465` — `tests`, 655 lines
- `crates/koushi-core/src/timeline/residency.rs:962` — `tests`, 300 lines
- `crates/koushi-core/src/timeline/room_key_recovery.rs:1595` — `tests`, 1170 lines
- `crates/koushi-core/src/timeline/thread_projection.rs:1847` — `tests`, 1758 lines
- `crates/koushi-diagnostics/src/lib.rs:622` — `tests`, 388 lines
- `crates/koushi-sdk/src/e2ee.rs:1888` — `device_cleanup_tests`, 236 lines
- `crates/koushi-sdk/src/e2ee.rs:2653` — `secure_backup_inspection_tests`, 285 lines
- `crates/koushi-sdk/src/e2ee.rs:2939` — `e2ee_trust_tests`, 821 lines
- `crates/koushi-sdk/src/e2ee.rs:4136` — `current_session_status_tests`, 316 lines
- `crates/koushi-sdk/src/e2ee.rs:6495` — `initial_share_diagnostics_tests`, 309 lines
- `crates/koushi-sdk/src/room_operations.rs:1454` — `tests`, 558 lines
- `crates/koushi-sdk/src/room_projection.rs:3199` — `tests`, 849 lines
- `crates/koushi-sdk/src/room_projection.rs:687` — `space_member_projection_tests`, 273 lines
- `crates/koushi-sdk/src/sync.rs:385` — `tests`, 486 lines
- `crates/koushi-state/src/reducer/mod.rs:2507` — `tests`, 1606 lines

## Source-contract mapping: koushi-state and koushi-sdk

All rows below had their Rust-source assertion transferred to the named checker rule. “Removed” means the old test was source-only; “retained” means its behavioral/compile-time portion keeps the original Rust test identity.

| Old file:test | Include/assertion facts | Checker rule | Disposition |
| --- | ---: | --- | --- |
| `focused_context_state.rs:reducer_source_mentions_the_focused_context_state_machine` | 2 / focused reducer markers | `state.focused_context_reducer_contract` | Removed |
| `sync_state.rs:production_state_has_no_legacy_sync_mode_vocabulary` | 6 / forbidden vocabulary | `state.no_legacy_sync_mode_vocabulary` | Removed |
| `password-login-smoke.rs:store_backed_restore_does_not_escape_its_runtime` | 1 / forbidden blocking restore | `sdk.password_smoke_runtime_safety` | Removed |
| `password-login-smoke.rs:store_backed_session_drop_enters_runtime_context` | 1 / runtime-enter and session-take markers | `sdk.password_smoke_runtime_safety` | Removed |
| `client_session.rs:matrix_client_store_config_uses_the_required_key_for_sqlite_builder` | 1 / keyed bounded SQLite builder | `sdk.client_store_config_contract` | Retained |
| `client_session.rs:desktop_client_builder_defaults_enable_threads_share_history_and_readiness` | 1 / desktop builder defaults | `sdk.desktop_client_builder_defaults` | Removed |
| `client_session.rs:client_builder_defaults_download_backup_keys_after_decryption_failures` | 1 / backup download default | `sdk.backup_download_default` | Removed |
| `e2ee.rs:recovery_key_path_uses_sdk_signature_publication_only` | 1 / required and forbidden recovery paths | `sdk.recovery.uses_sdk_signature_publication` | Removed |
| `e2ee.rs:recovery_sdk_records_standard_signature_round_trip_diagnostics` | 2 / vendored SDK diagnostics and privacy | `sdk.recovery.signature_round_trip_contract` | Removed |
| `room_operations.rs:mark_room_as_read_sends_read_marker_with_private_receipt` | 1 / marker/private receipt path | `sdk.room_read_marker_contract` | Removed |
| `room_operations.rs:cancel_space_invite_validates_invite_membership_before_kicking` | 1 / ordered invite cancellation path | `sdk.space_invite_cancellation_contract` | Retained |
| `room_operations.rs:room_tag_operations_use_sdk_tag_methods` | 1 / SDK room-tag calls | `sdk.room_tag_methods` | Removed |
| `room_operations.rs:pin_operations_use_sdk_pinned_event_methods` | 1 / SDK pin/unpin calls | `sdk.pinned_event_methods` | Removed |
| `room_operations.rs:room_management_wrappers_use_settings_privacy_and_moderation_apis` | 1 / management call inventory | `sdk.room_management_methods` | Retained; the old `.user_can_invite(own_user_id)` check was vacuous because it matched only its own assertion, so the retained DTO permission assertion is the stronger replacement |
| `room_projection.rs:joined_room_list_prefers_async_direct_dm_detection` | 1 / direct detection/fallback | `sdk.room_projection.async_direct_detection` | Removed |
| `room_projection.rs:joined_room_list_snapshot_avoids_full_member_scans` | 1 / bounded room-list projection | `sdk.room_projection.no_full_member_scan` | Removed |
| `room_projection.rs:joined_room_list_dm_resolution_uses_account_data_cached_and_heroes_candidates` | 1 / bounded DM candidates | `sdk.room_projection.dm_resolution_candidates` | Removed |
| `room_projection.rs:space_member_ids_are_no_sync_and_space_only` | 1 / no-sync joined membership | `sdk.room_projection.space_member_ids_no_sync` | Removed |
| `room_projection.rs:joined_only_helpers_do_not_use_active_membership` | 1 / joined/invited membership split | `sdk.room_projection.joined_only_membership` | Removed |
| `room_projection.rs:space_lookup_failures_are_not_coerced_to_empty_observations` | 1 / error propagation | `sdk.room_projection.space_lookup_failures_propagate` | Removed |
| `room_projection.rs:failed_space_member_counts_are_reported_as_unavailable` | 1 / unavailable-count diagnostics | `sdk.room_projection.failed_counts_unavailable` | Removed |
| `room_projection.rs:matrix_room_member_summaries_still_scans_full_members` | 1 / full member-summary path | `sdk.room_projection.member_summaries_full_members` | Removed |
| `room_projection.rs:live_direct_account_data_loader_is_local_only` | 1 / local account-data path | `sdk.room_projection.direct_account_data_local_only` | Removed |
| `room_projection.rs:direct_account_data_dm_detection_fetches_server_when_store_misses` | 1 / server fallback path | `sdk.room_projection.direct_account_data_server_fallback` | Removed |
| `sync.rs:sliding_sync_invite_probe_contract_is_typed_bounded_and_discards_cursor` | 1 / timeout/request ordering and forbidden owners | `sdk.sync.sliding_sync_invite_probe_contract` | Removed |
| `send_backup_policy.rs:all_session_constructors_leave_the_per_send_backup_fence_disabled` | 12-source scan / exactly 3 `false`, 0 `true` | `sdk.sessions.no_per_send_backup_fence` | Removed; origin/main and this branch both contain the same 3 production calls (auth twice, client_session once); older plan prose saying 4 was stale |
| `send_backup_policy.rs:library_source_manifest_is_complete_and_unique` | 12 / complete unique source list | `sdk.library_source_manifest` | Removed |
| `timeline_gap_adapter.rs:committed_room_checkpoint_has_no_legacy_or_room_absent_api` | 12 / forbidden legacy checkpoint API | `sdk.timeline.committed_room_checkpoint_no_legacy_api` | Removed |

## Source-contract mapping: src-tauri

All src-tauri rows were source-only and were removed; the three non-Rust JSON embeddings remain in their behavioral serialization tests. Shared `commands_source`/`production_source` helpers were removed after their last structural consumer moved.

| Old file:test | Structural facts | Checker rule(s) |
| --- | --- | --- |
| `commands/activity.rs:open_activity_event_opens_anchored_main_timeline_without_room_resubscribe` | anchored-navigation required/forbidden markers | `desktop.activity.navigation_contract` |
| `commands/activity.rs:open_activity_event_waits_before_opening_anchored_event_timeline` | close→wait→select→open→anchor order | `desktop.activity.navigation_contract` |
| `commands/activity.rs:activity_tauri_command_contracts_are_present` | command, builder, registration inventory | `desktop.activity.command_contract` |
| `commands/contracts.rs:submit_core_command_does_not_hold_connection_mutex_while_awaiting_send` | bounded unlocked submit path | `desktop.commands.submit_core_command_contract` |
| `commands/contracts.rs:event_wait_loops_resync_on_lag_instead_of_failing_immediately` | lag-tolerant event waiters | `desktop.commands.event_wait_lag_contract` |
| `commands/contracts.rs:correlated_operation_failures_preserve_core_failure_kind_in_invoke_errors` | typed failure waiter mapping | `desktop.commands.failure_waiter_contract` |
| `commands/contracts.rs:every_tauri_command_is_registered_in_generate_handler` | exhaustive command registration | `desktop.commands.tauri_command_registration` |
| `commands/diagnostics.rs:diagnostic_snapshot_command_is_registered_in_generate_handler` | diagnostics registration | `desktop.commands.tauri_command_registration` |
| `commands/diagnostics.rs:viewport_sync_command_is_registered_in_generate_handler` | viewport registration | `desktop.commands.tauri_command_registration` |
| `commands/directory.rs:start_direct_message_selects_the_resolved_room_before_returning` | DM started→room projection→selection order | `desktop.directory.start_dm_contract` |
| `commands/directory.rs:join_directory_room_waits_for_backend_selected_room` | joined-room→selection order and timeout | `desktop.directory.join_room_selection_contract` |
| `commands/e2ee.rs:e2ee_trust_tauri_command_contracts_are_present` | E2EE commands/builders/registrations | `desktop.e2ee.command_contract` |
| `commands/local_encryption.rs:credential_health_tauri_command_contract_is_present` | probe command/builder/registration | `desktop.local_encryption.command_contract` |
| `commands/local_encryption.rs:reset_local_data_tauri_command_contract_is_present` | reset command/wait/registration | `desktop.local_encryption.command_contract` |
| `commands/navigation.rs:select_room_uses_core_selection_settlement_without_resubscribing_timeline` | Core-owned selection path | `desktop.navigation.command_contract` |
| `commands/navigation.rs:room_transition_and_backfill_commands_emit_submit_trace_tokens` | private-safe submit traces | `desktop.navigation.command_contract` |
| `commands/navigation.rs:select_search_result_selects_room_then_enters_anchored_timeline_without_room_resubscribe` | anchored search navigation | `desktop.navigation.command_contract` |
| `commands/navigation.rs:close_focused_context_command_routes_to_app_close_focused_context` | close-focused command route | `desktop.navigation.command_contract` |
| `commands/navigation.rs:close_focused_context_command_waits_until_main_timeline_is_live` | close→wait→snapshot order | `desktop.navigation.command_contract` |
| `commands/navigation.rs:select_space_command_records_private_data_free_transition_trace` | space trace order/fields | `desktop.navigation.space_trace_contract` |
| `commands/profile.rs:profile_tauri_command_contracts_are_present` | profile commands/builders/registrations | `desktop.profile.command_contract` |
| `commands/room.rs:room_management_tauri_commands_wait_for_correlated_core_events` | room and space correlated waiters/registrations | `desktop.room.operation_wait_contract`, `desktop.room.space_operation_contract` |
| `commands/search.rs:search_scope_resolution_preserves_non_all_scope_contract` | room/space scope and no global collapse | `desktop.search.command_contract` |
| `commands/search.rs:submit_search_returns_after_correlated_search_start_before_result_completion` | request allocation and submit→wait order | `desktop.search.command_contract` |
| `commands/session.rs:submit_login_request_waits_for_authenticated_session_and_leaves_sync_to_runtime_effects` | login waiter/timeout and no adapter sync start | `desktop.session.login_wait_contract` |
| `commands/settings.rs:update_settings_tauri_command_contract_is_present` | settings route/registration | `desktop.settings.command_contract` |
| `commands/settings.rs:rebuild_search_index_tauri_command_contract_is_present` | rebuild route/registration | `desktop.settings.command_contract` |
| `commands/timeline.rs:acknowledge_timeline_batch_rendered_routes_every_generation_fence` | generation-fenced ACK | `desktop.timeline.generation_ack_contract` |
| `commands/timeline.rs:composer_key_resolver_command_contract_is_present` | Rust resolver route/registration | `desktop.timeline.command_contract` |
| `commands/timeline.rs:reaction_tauri_command_contracts_are_present` | reaction trace/registration | `desktop.timeline.signal_contract` |
| `commands/timeline.rs:read_signal_tauri_commands_emit_latency_trace_tokens` | receipt/fully-read traces | `desktop.timeline.signal_contract` |
| `commands/timeline.rs:scheduled_send_tauri_command_contracts_are_present` | scheduled-send commands/builders | `desktop.timeline.scheduled_send_contract` |
| `commands/timeline.rs:send_queue_tauri_command_contracts_are_present` | retry/cancel commands/builders | `desktop.timeline.send_queue_contract` |
| `commands/timeline.rs:thread_timeline_backwards_pagination_contract_is_present` | thread pagination builder/registration | `desktop.timeline.command_contract` |
| `core_event_forwarder.rs:lag_resync_forwarder_requests_core_timeline_replay_after_marker` | owned ordered lag replay | `desktop.forwarder.lag_recovery_contract` |
| `lib.rs:qa_control_pipe_env_is_debug_or_test_only` | direct cfg gate | `desktop.native.qa_control_pipe_cfg` |
| `lib.rs:macos_close_requested_hides_without_stopping_background_tasks` | hide-not-stop close policy | `desktop.native.window_lifecycle_contract` |
| `lib.rs:single_instance_reopen_shows_existing_main_window` | single-instance reopen path | `desktop.native.reopen_contract` |
| `lib.rs:macos_run_event_reopen_shows_existing_main_window` | macOS reopen path | `desktop.native.reopen_contract` |
| `viewport_sync.rs:native_access_and_recovery_mechanisms_are_isolated` | no resize/DOM dispatch fallback | `desktop.viewport.native_adapter_isolation` |

## Complete account source-contract mapping

This is the complete account-module ledger for the migration. The eight rows
from the earlier account slice are repeated here so every account file is
covered in one table. Include counts are the old `include_str!` invocations;
all mapped assertions are now owned by the named checker rule. `Removed` means
source-only test deletion. No mixed tests occurred in this slice, so no
behavioral or compile assertion was removed. No additional vacuous self-match
was found in the account mappings.

| Account file:test | Source includes | Checker rule | Disposition |
| --- | ---: | --- | --- |
| `account_work.rs:(none)` | 0 | — | No source contract |
| `account_management.rs:session_replacement_uses_the_teardown_that_aborts_discovery` | 2 | `core.account.session_replacement_teardown` | Removed (prior slice) |
| `actor.rs:account_actor_reducer_actions_use_reliable_delivery` | 13 | `core.account.reliable_reducer_delivery` | Removed (prior slice) |
| `local_data_cleanup.rs:(none)` | 0 | — | No source contract |
| `profile.rs:login_success_is_not_blocked_by_optional_account_hydration` | 2 | `core.account.login_hydration_order` | Removed (prior slice) |
| `profile.rs:async_account_hydration_is_generation_gated` | 2 | `core.account.hydration_generation_fence` | Removed (prior slice) |
| `profile.rs:local_user_alias_failure_reconciles_authoritative_aliases` | 1 | `core.account.alias_failure_reconciliation` | Removed (prior slice) |
| `recovery_backup.rs:secure_backup_monitor_has_one_sixty_second_timer_owner` | 5 | `core.account.secure_backup_monitor_owner` | Removed (prior slice) |
| `recovery_backup.rs:e2ee_key_management_failures_use_typed_classification` | 5 | `core.account.e2ee_typed_failure_classification` | Removed (prior slice) |
| `recovery_backup.rs:submit_recovery_hydrates_joined_room_keys_after_secret_recovery` | 2 | `core.account.recovery_key_hydration_order` | Removed (prior slice) |
| `routing.rs:search_crawler_room_notifications_are_latest_wins_and_nonblocking` | 1 | `core.account.crawler_notification_latest_wins` | Removed |
| `routing.rs:sync_stop_command_must_not_spawn_missing_sync_actor` | 1 | `core.account.sync_stop_routing` | Removed |
| `routing.rs:manual_sync_once_rejection_precedes_sync_actor_spawn_and_send` | 1 | `core.account.manual_sync_once_guard` | Removed |
| `runtime_children.rs:session_established_handoff_to_room_actor_is_reliable` | 1 | `core.account.session_established_handoff` | Removed |
| `scheduled_send.rs:secure_backup_barrier_covers_normal_and_scheduled_user_content_routes` | 5 | `core.account.secure_backup_content_barrier` | Removed |
| `scheduled_send.rs:local_scheduled_room_send_does_not_use_per_session_backup_durability_fence` | 1 | `core.account.local_scheduled_send_no_backup_fence` | Removed |
| `session_lifecycle.rs:explicit_logout_selects_the_non_preserving_teardown_path` | 2 | `core.account.explicit_logout_teardown` | Removed |
| `session_lifecycle.rs:restore_into_store_emits_event_cache_status_without_failing_restore` | 3 | `core.account.restore_event_cache_status` | Removed |
| `session_lifecycle.rs:changing_homeserver_does_not_logout_pending_login_on_the_old_server` | 2 | `core.account.homeserver_change_login_abort` | Removed |
| `session_lifecycle.rs:authentication_completion_installs_quarantine_before_ready_side_effects` | 4 | `core.account.authentication_quarantine` | Removed |
| `session_lifecycle.rs:restore_trace_covers_startup_restore_boundaries_without_private_ids` | 4 | `core.account.restore_trace` | Removed |
| `session_lifecycle.rs:verification_restore_diagnostics_separate_trust_timing_from_persistence` | 3 | `core.account.restore_diagnostics` | Removed |
| `session_lifecycle.rs:password_login_is_persistent_store_first_without_saved_fallback` | 1 | `core.account.password_store_first` | Removed |
| `session_lifecycle.rs:session_change_observer_routes_unknown_token_to_session_lock` | 3 | `core.account.session_change_observer` | Removed |
| `session_lifecycle.rs:soft_logout_reauth_keeps_locked_session_until_password_login_succeeds` | 2 | `core.account.soft_logout_reauth` | Removed |
| `session_lifecycle.rs:account_actor_credential_store_hot_paths_use_blocking_port` | 5 | `core.account.credential_store_blocking` | Removed |
| `sliding_sync.rs:(none)` | 0 | — | No source contract |
| `trust_gate.rs:secure_backup_queue_latch_follows_authoritative_gate_lifecycle` | 4 | `core.account.secure_backup_latch` | Removed |
| `trust_gate.rs:session_status_refresh_task_is_cancelled_with_the_session_runtime` | 1 | `core.account.session_status_refresh_teardown` | Removed |
| `trust_gate.rs:provisional_pre_first_response_failure_retries_under_the_same_owner` | 1 | `core.account.provisional_sync_retry` | Removed |
| `trust_gate.rs:provisional_first_response_is_published_only_after_actor_delivery` | 1 | `core.account.provisional_sync_first_response` | Removed |
| `trust_gate.rs:admission_timeout_is_cancelled_with_the_provisional_runtime` | 1 | `core.account.admission_timeout_teardown` | Removed |
| `trust_gate.rs:provisional_verification_uses_encryption_sync_service` | 1 | `core.account.provisional_encryption_sync_service` | Removed |
| `trust_gate.rs:qa_device_key_refresh_queries_before_asserting_the_exact_device` | 1 | `core.account.qa_device_key_refresh` | Removed |
| `trust_gate.rs:verification_method_discovery_completion_projects_without_awaiting_sender_task` | 1 | `core.account.verification_discovery_completion` | Removed |
| `verification.rs:sas_handle_adoption_is_classified_before_any_active_flow_side_effect` | 1 | `core.account.sas_adoption` | Removed |
| `verification.rs:incoming_actor_admission_checks_own_user_before_replacing_runtime` | 1 | `core.account.incoming_verification_admission` | Removed |
| `verification.rs:identity_reset_auth_wait_has_cancel_and_timeout_exits` | 6 | `core.account.identity_reset_auth_lifecycle` | Removed |

Slice result: 58 account-scoped Rust-source includes removed from 27
source-only tests. Mixed tests retained: 0. Account-scoped Rust-source includes
remaining: 0. `runSourceContractRules`: 0 violations.

## Source-contract mapping: koushi-core runtime/search/sync/thread slices (#753 continuation)

The scoped diff contains 46 deleted source-only `#[test]` identities. Rule
names are the existing `runSourceContractRules` contracts; no Rust/checker code
is changed by this continuation.

| Old file:test | Include/assertion facts summary | Checker rule | Disposition |
| --- | --- | --- | --- |
| `runtime.rs:role_command_reduces_pending_before_one_account_route` | 1 include / pending-before-single-account-route and exactly-one route | `core.runtime.role_command_pending_route` | Removed |
| `runtime.rs:activity_mark_read_routes_persistent_room_mark_read_commands` | 1 include / persistent mark-read, internal request, marker update | `core.runtime.activity_mark_read_route` | Removed |
| `runtime.rs:open_thread_command_must_execute_thread_timeline_effects` | 1 include / OpenThread effect is executed, not discarded | `core.runtime.thread_effect_execution` | Removed |
| `runtime.rs:runtime_must_execute_start_sync_effects_from_session_reducer` | 1 include / StartSync routes to SyncCommand::Start | `core.runtime.start_sync_effect_execution` | Removed |
| `runtime.rs:runtime_must_execute_session_cleanup_effects_from_session_reducer` | 1 include / StopSync routes to SyncCommand::Stop in both lanes | `core.runtime.session_cleanup_effect_execution` | Removed |
| `runtime.rs:runtime_routes_current_device_trust_rechecks_in_both_effect_lanes` | 1 include / trust recheck reaches AccountActor in both lanes | `core.runtime.trust_recheck_effect_execution` | Removed |
| `runtime.rs:runtime_routes_current_session_status_in_both_effect_lanes` | 1 include / session-status route in both effect lanes | `core.runtime.session_status_effect_execution` | Removed |
| `runtime.rs:app_actor_persistence_uses_blocking_store_port` | 4 includes / all persistence paths use executor blocking port | `core.runtime.persistence_blocking_port` | Removed |
| `runtime.rs:runtime_must_execute_subscribe_timeline_effects_from_navigation_reducers` | 1 include / SubscribeTimeline room effect is executed | `core.runtime.subscribe_timeline_effect` | Removed |
| `runtime.rs:runtime_room_selection_replays_existing_room_timeline_for_empty_renderer_store` | 1 include / selected room replays existing timeline | `core.runtime.navigation_replay` | Removed |
| `runtime.rs:closed_account_actor_timeline_route_is_not_reported_as_queue_overflow` | 1 include / closed route preserves non-overflow failure | `core.runtime.closed_timeline_route` | Removed |
| `runtime.rs:actor_projection_start_sync_effects_must_not_be_discarded` | 1 include / projected StartSync is handled | `core.runtime.actor_start_sync_effect` | Removed |
| `runtime.rs:runtime_sync_trace_covers_start_sync_effect_boundaries` | 1 include / StartSync trace covers both effect boundaries | `core.runtime.sync_trace` | Removed |
| `runtime.rs:opening_a_replacement_thread_unsubscribes_the_previous_thread_before_subscribe` | 1 include / replacement unsubscribe precedes subscribe | `core.runtime.thread_replacement` | Removed |
| `runtime.rs:opening_a_replacement_focused_context_unsubscribes_previous_focused_before_subscribe` | 1 include / focused replacement unsubscribe precedes subscribe | `core.runtime.focused_replacement` | Removed |
| `runtime.rs:opening_focused_context_repairs_target_event_cache_before_subscribe` | 1 include / target cache repair precedes focused subscribe | `core.runtime.focused_cache_repair` | Removed |
| `runtime.rs:selecting_a_replacement_room_cancels_previous_room_pagination_before_subscribe` | 1 include / old-room pagination cancellation precedes subscribe | `core.runtime.room_switch_pagination` | Removed |
| `runtime.rs:selecting_a_replacement_room_cancels_previous_room_link_previews_before_subscribe` | 1 include / old-room link-preview cancellation precedes subscribe | `core.runtime.room_switch_link_previews` | Removed |
| `runtime.rs:focused_ack_and_command_coalescer_share_the_latest_published_baseline` | 1 include / coalescer and focused ACK use published baseline | `core.runtime.coalescer_baseline` | Removed |
| `runtime.rs:timestamp_jump_uses_local_activity_projection_before_homeserver_fallback` | 1 include / local activity resolution precedes server fallback | `core.runtime.timestamp_activity_projection` | Removed |
| `runtime/connection.rs:core_connection_command_handle_clones_submit_path` | 1 include / cloned bounded command submit path and delegation | `core.runtime.connection_command_handle` | Removed |
| `executor.rs:executor_exposes_blocking_task_port` | 1 include / public blocking port maps to Tokio blocking pool | `core.runtime.executor_blocking_port` | Removed |
| `renderable_thumbnail.rs:avatar_and_preview_thumbnail_helpers_do_not_use_legacy_plaintext_paths` | 2 includes / encrypted media fetch; no legacy path or file URL | `core.runtime.thumbnail_paths` | Removed |
| `search.rs:search_query_failures_are_classified_from_sdk_error` | 1 include / query handler classifies SDK failure | `core.search.query_failure_classification` | Removed |
| `search.rs:search_actor_handles_new_queries_before_crawl_and_sdk_completions` | 1 include / biased query priority and stale-task cancellation | `core.search.query_priority` | Removed |
| `search.rs:empty_query_is_not_special_cased_in_runtime` | 2 includes / empty-query ownership remains in search actor | `core.search.empty_query_ownership` | Removed |
| `search.rs:search_actor_crawler_uses_element_style_round_robin_checkpoints` | 1 include / queued checkpoints, one page, unfinished requeue | `core.search.crawler_round_robin` | Removed |
| `search.rs:search_actor_prunes_crawler_queue_when_joined_rooms_change` | 1 include / queue prune and retired in-flight abort | `core.search.crawler_pruning` | Removed |
| `search.rs:search_actor_history_crawler_uses_account_wide_account_work` | 1 include / shared account-wide crawl scheduler | `core.search.crawler_account_work` | Removed |
| `search.rs:search_actor_room_availability_notifications_have_nonblocking_entrypoint` | 1 include / latest-wins nonblocking notification, closed delivery not success | `core.search.availability_nonblocking` | Removed |
| `search.rs:search_crawler_lifecycle_projects_actor_owned_stop_settles` | 1 include / start/stop/prune settle actor-owned crawler state | `core.search.crawler_lifecycle` | Removed |
| `search.rs:preempted_crawl_page_is_requeued` | 1 include / Preempted page requeues checkpoint at front | `core.search.preempted_page_requeue` | Removed |
| `search.rs:automatic_crawl_starts_are_delayed_at_startup` | 1 include / startup delay gates automatic, not manual, starts | `core.search.startup_delay` | Removed |
| `search_crawler.rs:history_crawler_page_runner_fetches_only_one_messages_page` | 1 include / exactly one messages page, no history loop | `core.search.page_single_fetch` | Removed |
| `search_crawler.rs:history_crawler_page_runner_acquires_the_search_crawl_work_kind` | 1 include / scheduler permit precedes page fetch | `core.search.page_work_kind` | Removed |
| `search_crawler.rs:crawler_page_emits_startup_trace` | 1 include / crawler page has startup trace phase | `core.search.page_startup_trace` | Removed |
| `search_crawler.rs:crawler_page_yields_to_timeline_via_cancellation` | 1 include / cancellation yields Preempted and traces it | `core.search.page_cancellation` | Removed |
| `sync.rs:sync_service_has_one_all_rooms_owner` | 1 include / one SyncService all-rooms owner; no legacy backend paths | `core.sync.single_all_rooms_owner` | Removed |
| `sync.rs:running_state_is_not_the_committed_response_handoff` | 1 include / committed response handoff is explicit and range-independent | `core.sync.committed_response_handoff` | Removed |
| `sync.rs:latest_observed_commit_is_forwarded_to_timeline_before_range_readiness` | 1 include / commit forwards timeline before readiness | `core.sync.timeline_commit_before_readiness` | Removed |
| `sync.rs:terminated_sync_owner_is_restarted_instead_of_settled_failed` | 1 include / terminated owner replacement restarts service | `core.sync.terminated_owner_restart` | Removed |
| `threads_list.rs:aggregate_refresh_has_production_manager_start_and_finish_callers` | 2 includes / manager start/finish callers and projection scheduling | `core.threads.aggregate_refresh_callers` | Removed |
| `threads_list.rs:thread_root_projection_source_never_uses_room_pagination_or_anchor_materialization` | 1 include / root projection forbids pagination and anchor materialization | `core.threads.root_projection_no_pagination` | Removed |
| `threads_list.rs:open_subscription_loads_initial_page_before_emitting_opened` | 1 include / initial page precedes Opened | `core.threads.open_subscription_initial_page` | Removed |
| `threads_list.rs:paginate_updates_are_correlated_to_paginate_request_id` | 1 include / pagination update uses current request ID | `core.threads.pagination_request_correlation` | Removed |
| `threads_list.rs:thread_list_relays_are_reliable_and_paginate_errors_fail` | 1 include / reliable relays and explicit classified pagination failure | `core.threads.reliable_relays` | Removed |
| `send_diagnostics.rs:distinguishes_http_timeouts_without_exposing_transport_details` | 1 include / reqwest timeout classifier source marker | `core.runtime.send_http_timeout` | Retained mixed test; behavioral timeout assertions unchanged |

**Verification counts:** deleted `#[test]` identities in the scoped diff: 46;
source-contract mapping rows: 47 (46 removed + 1 retained mixed); missing rows:
0; extra rows: 0. No production behavior or behavioral identity changed.

## Source-contract mapping: koushi-core room/store slice (#753 continuation)

All rows below are deleted source-only tests from the current scoped diff. Rule
names are the existing `core.room` / `core.store` checker contracts.

| Old fully-qualified test identity | Source includes | Checker rule | Disposition |
| --- | ---: | --- | --- |
| `koushi_core::room::actor::tests::room_actor_command_loop_never_awaits_room_list_refresh` | 1 | `core.room.actor_command_loop` | Removed |
| `koushi_core::room::actor::tests::sync_started_requires_one_live_room_list_service` | 1 | `core.room.sync_started_owner` | Removed |
| `koushi_core::room::directory::tests::directory_join_selects_room_before_room_joined_event_is_emitted` | 1 | `core.room.directory_join_order` | Removed |
| `koushi_core::room::list_observer::tests::live_direct_observer_subscribes_before_cached_account_data_read` | 1 | `core.room.live_direct_subscription_order` | Removed |
| `koushi_core::room::list_observer::tests::room_list_runtime_has_no_legacy_or_base_client_projection_path` | 1 | `core.room.list_no_legacy_projection` | Removed |
| `koushi_core::room::list_observer::tests::room_list_observation_relays_parent_only_space_links_before_projection` | 1 | `core.room.list_relay_order` | Removed |
| `koushi_core::room::list_observer::tests::room_list_projection_updates_known_book_before_reliable_delivery` | 1 | `core.room.list_known_book_delivery` | Removed |
| `koushi_core::room::mentions::tests::existing_membership_change_message_routes_to_space_refresh` | 1 | `core.room.mention_membership_refresh` | Removed |
| `koushi_core::room::operations::tests::mark_room_as_read_success_updates_fully_read_marker_before_clearing_counts` | 1 | `core.room.mark_read_order` | Removed |
| `koushi_core::room::operations::tests::room_tag_success_path_does_not_refresh_from_stale_sdk_snapshot` | 2 | `core.room.tag_no_stale_refresh` | Removed |
| `koushi_core::room::operations::tests::create_room_links_parent_space_child_with_created_room_id_before_completion_event` | 2 | `core.room.create_links_before_completion` | Removed |
| `koushi_core::room::operations::tests::missing_space_child_repairs_are_actor_owned_and_retryable` | 2 | `core.room.missing_space_child_repair` | Removed |
| `koushi_core::room::pins::tests::pin_success_settles_pending_before_pinned_projection_reload` | 1 | `core.room.pin_settlement_order` | Removed |
| `koushi_core::room::pins::tests::pin_and_unpin_commands_require_actor_known_room_guard_before_sdk_call` | 1 | `core.room.pin_command_guard` | Removed |
| `koushi_core::room::space_members::tests::space_member_load_failure_does_not_construct_an_empty_projection` | 1 | `core.room.space_member_failure_projection` | Removed |
| `koushi_core::room::space_members::tests::background_space_member_lookup_failure_preserves_state_and_only_records_diagnostic` | 1 | `core.room.space_member_background_failure` | Removed |
| `koushi_core::room::space_members::tests::cancel_space_invite_reconciles_a_fresh_projection_before_settling` | 1 | `core.room.space_invite_cancellation_order` | Removed |
| `koushi_core::store::credential_backend::tests::file_credential_store_is_available_to_release_qa_binary_only` | 1 | `core.store.file_credential_cfg` | Removed |

**Verification counts:** deleted identities in the current scoped diff: 18;
mapping rows: 18; missing: 0; extra: 0. No Rust or checker source was edited.

## Complete source-contract mapping: koushi-core timeline slice (#753 continuation)

The current scoped diff deletes 52 timeline source-only `#[test]` identities. The 53 existing `core.timeline` rule constants are listed below; the one rule without a deleted identity is explicitly reported rather than fabricated into a mapping.

| Old fully-qualified test identity | Checker rule | Disposition |
| --- | --- | --- |
| `koushi_core::timeline::actor::tests::room_unsubscribe_clears_projection_service_before_dropping_the_actor` | `core.timeline.unsubscribe_cleanup_order` | Removed |
| `koushi_core::timeline::diagnostics::tests::timeline_subscribe_and_paginate_emit_startup_trace` | `core.timeline.startup_trace` | Removed |
| `koushi_core::timeline::diagnostics::tests::timeline_route_and_paginate_emit_ordered_trace_tokens` | `core.timeline.trace_tokens` | Removed |
| `koushi_core::timeline::gap_repair::tests::terminal_gap_repair_failures_resume_queued_candidate_inspection` | `core.timeline.gap_repair_failure_resume` | Removed |
| `koushi_core::timeline::gap_repair::tests::terminal_gap_inspection_paths_resume_queued_work_before_release_wake` | `core.timeline.gap_inspection_resume` | Removed |
| `koushi_core::timeline::gap_repair::tests::gap_repair_takes_a_scheduler_permit_around_one_bounded_batch` | `core.timeline.gap_repair_scheduler` | Removed |
| `koushi_core::timeline::item_projection::tests::profile_change_projection_does_not_emit_user_id_body` | `core.timeline.profile_change_projection` | Removed |
| `koushi_core::timeline::item_projection::tests::timeline_search_index_mutations_use_reliable_delivery` | `core.timeline.search_reliable_delivery` | Removed |
| `koushi_core::timeline::item_projection::tests::media_gallery_and_thread_attention_projections_use_reliable_delivery` | `core.timeline.media_attention_reliable_delivery` | Removed |
| `koushi_core::timeline::item_projection::tests::retry_send_reenables_sdk_room_queue_before_unwedge` | `core.timeline.retry_queue_order` | Removed |
| `koushi_core::timeline::item_projection::tests::cancel_send_reenables_sdk_room_queue_after_abort` | `core.timeline.cancel_queue_order` | Removed |
| `koushi_core::timeline::item_projection::tests::reaction_and_read_signal_handlers_emit_private_latency_traces` | `core.timeline.signal_traces` | Removed |
| `koushi_core::timeline::item_projection::tests::timeline_link_preview_load_emits_private_data_free_trace_tokens` | `core.timeline.link_preview_trace` | Removed |
| `koushi_core::timeline::item_projection::tests::timeline_link_preview_fetches_do_not_block_actor_command_queue` | `core.timeline.link_preview_off_loop` | Removed |
| `koushi_core::timeline::item_projection::tests::timeline_link_preview_fetches_are_abortable_without_dropping_the_actor` | `core.timeline.link_preview_cancellation` | Removed |
| `koushi_core::timeline::item_projection::tests::initial_timeline_items_are_forwarded_to_search_index` | `core.timeline.initial_search_forward` | Removed |
| `koushi_core::timeline::manager::tests::room_subscribe_success_reduces_timeline_subscribed_action` | `core.timeline.subscribe_success` | Removed |
| `koushi_core::timeline::manager::tests::timeline_subscribe_settles_use_reliable_reducer_actions` | `core.timeline.subscribe_reliable_settles` | Removed |
| `koushi_core::timeline::manager::tests::thread_timeline_focus_uses_sdk_thread_pagination` | `core.timeline.thread_focus` | Removed |
| `koushi_core::timeline::manager::tests::timeline_subscribe_is_idempotent_for_existing_key` | `core.timeline.idempotent_subscribe` | Removed |
| `koushi_core::timeline::manager::tests::sync_started_subscribes_existing_timeline_rooms_with_live_room_list_service` | `core.timeline.sync_started_rebuild` | Removed |
| `koushi_core::timeline::manager::tests::timeline_ensure_subscribed_can_skip_existing_actor_replay` | `core.timeline.ensure_subscribed` | Removed |
| `koushi_core::timeline::manager::tests::replay_subscribed_recovery_replays_initial_items_causeless_for_all_timelines` | `core.timeline.replay_subscribed` | Removed |
| `koushi_core::timeline::media::tests::media_downloads_spawn_bounded_tasks_and_report_all_exits` | `core.timeline.media_download_lifecycle` | Removed |
| `koushi_core::timeline::media::tests::media_downloads_diagnose_stage_and_failure_boundaries` | `core.timeline.media_download_diagnostics` | Removed |
| `koushi_core::timeline::navigation::tests::timeline_pagination_uses_the_account_work_scheduler` | `core.timeline.pagination_scheduler` | Removed |
| `koushi_core::timeline::navigation::tests::timeline_pagination_is_abortable_without_dropping_the_actor` | `core.timeline.pagination_cancellation` | Removed |
| `koushi_core::timeline::navigation::tests::pagination_terminal_is_emitted_after_active_task_release` | `core.timeline.pagination_terminal_release` | Removed |
| `koushi_core::timeline::navigation::tests::restore_anchor_handler_is_room_only_and_bounded` | `core.timeline.restore_room_bounded` | Removed |
| `koushi_core::timeline::navigation::tests::restore_anchor_budget_respects_frontend_hint` | `core.timeline.restore_budget` | Removed |
| `koushi_core::timeline::navigation::tests::restore_walk_coalesces_items_updated_to_single_flush` | `core.timeline.restore_coalescing` | Removed |
| `koushi_core::timeline::navigation::tests::restore_terminal_is_anchor_present_not_timing_dependent` | `core.timeline.restore_terminal` | Removed |
| `koushi_core::timeline::outbound_send::tests::send_enqueue_takes_the_interactive_guard_before_the_sdk_enqueue` | `core.timeline.send_admission_guard` | Removed |
| `koushi_core::timeline::outbound_send::tests::send_completion_keeps_the_interactive_guard_until_terminal` | `core.timeline.send_completion_guard` | Removed |
| `koushi_core::timeline::outbound_send::tests::send_submission_is_not_reduced_before_manager_worker_route_exists` | `core.timeline.send_submission_route` | Removed |
| `koushi_core::timeline::outbound_send::tests::thread_reply_submission_is_not_reduced_before_manager_worker_route_exists` | `core.timeline.thread_reply_submission_route` | Removed |
| `koushi_core::timeline::outbound_send::tests::thread_timeline_keys_project_send_reply_to_thread_composer_actions` | `core.timeline.thread_composer_route` | Removed |
| `koushi_core::timeline::outbound_send::tests::outbound_send_state_uses_sdk_truth_and_reliable_settles` | `core.timeline.outbound_state` | Removed |
| `koushi_core::timeline::outbound_send::tests::outbound_sdk_enqueues_are_session_manager_owned_and_supervised` | `core.timeline.send_queue_supervision` | Removed |
| `koushi_core::timeline::read_state::tests::set_fully_read_success_uses_private_read_receipt_before_clearing_room_unread_summary` | `core.timeline.room_read_marker` | Removed |
| `koushi_core::timeline::read_state::tests::send_read_receipt_uses_threaded_receipt_for_thread_timelines` | `core.timeline.thread_read_receipts` | Removed |
| `koushi_core::timeline::read_state::tests::manager_read_completion_lane_precedes_ordinary_mailbox` | `core.timeline.read_completion_priority` | Removed |
| `koushi_core::timeline::read_state::tests::replaying_thread_initial_items_preserves_semantic_attention_tracker` | `core.timeline.replay_attention` | Removed |
| `koushi_core::timeline::read_state::tests::timeline_builder_does_not_track_state_event_read_receipts` | `core.timeline.receipt_tracking` | Removed |
| `koushi_core::timeline::read_state::tests::production_receipt_diff_path_uses_fenced_ordered_observation_delivery` | `core.timeline.receipt_observation_delivery` | Removed |
| `koushi_core::timeline::read_state::tests::initial_receipts_use_the_ordered_local_profile_observation_batch` | `core.timeline.initial_receipt_observation` | Removed |
| `koushi_core::timeline::read_state::tests::authoritative_recovery_receipts_use_the_same_ordered_observation_batch` | `core.timeline.recovery_receipt_observation` | Removed |
| `koushi_core::timeline::relay::tests::timeline_subscribe_spawns_always_on_origin_observer` | `core.timeline.origin_observer` | Removed |
| `koushi_core::timeline::room_key_recovery::tests::room_key_reshare_handler_does_not_hold_the_manager_on_sdk_work` | `core.timeline.room_key_reshare` | Removed |
| `koushi_core::timeline::thread_projection::tests::room_live_timeline_focus_includes_threaded_events` | `core.timeline.room_focus` | Removed |
| `koushi_core::timeline::thread_projection::tests::sdk_projection_reads_thread_contract_accessors` | `core.timeline.sdk_projection_accessors` | Removed |
| `koushi_core::timeline::thread_projection::tests::recovery_and_manager_owned_receipt_success_preserve_attention_ordering` | `core.timeline.receipt_attention_ordering` | Removed |

**Verification counts:** deleted `#[test]` identities: 52; mapping rows: 52; missing rows: 0; extra rows: 0. The existing rule `core.timeline.thread_root_hydration` has no corresponding deleted identity in the current diff (unmatched existing rule: 1); it is not counted as a mapping row. No mixed tests were found: all 52 deleted identities were source-only, so no behavioral assertion required restoration. Vacuous self-matches: 0. No Rust or checker source was edited.

## Final source-contract slice: headless QA contracts and integration tests

The current diff deletes 46 source-only tests from `headless_core_qa::contracts`,
one source-only `login_store_contracts` test that depended on the removed source
helpers, and 10 source-only integration tests. `Missing: 0; extra: 0; vacuous: 0;
mixed: 0`. The 46 QA mappings are listed as exact identity → existing rule →
facts (all dispositions: **Removed**):

| Old identity | Existing rule | Facts |
| --- | --- | --- |
| `headless_core_qa::contracts::active_reconnect_uses_encryption_gate_before_timeline_work` | `core.qa.reconnect_encryption_gate` | encryption gate precedes timeline work |
| `headless_core_qa::contracts::invite_timeout_uses_private_safe_observer_diagnostic_summary` | `core.qa.private_safe_invite_timeout` | invite timeout omits room identity |
| `headless_core_qa::contracts::production_qa_never_overlaps_actor_owned_sync_with_manual_sync_once` | `core.qa.no_manual_sync_once` | no manual SyncOnce |
| `headless_core_qa::contracts::owner_driven_e2ee_body_waiter_keeps_the_extended_deadline` | `core.qa.e2ee_waiter_deadline` | fixed E2EE waiter deadline |
| `headless_core_qa::contracts::unverified_peer_refreshes_device_keys_before_behavioral_checkpoints` | `core.qa.multi_device_order` | refresh precedes send/checkpoints |
| `headless_core_qa::contracts::e2ee_key_delivery_preestablishes_invite_before_optional_b_login` | `core.qa.invite_before_optional_login` | invite precedes optional login |
| `headless_core_qa::contracts::headless_qa_binary_initializes_rust_log_tracing` | `core.qa.tracing_and_device_labels` | tracing and private-safe labels |
| `headless_core_qa::contracts::e2ee_strict_qa_keeps_actor_owned_sync_running_for_multi_device_send` | `core.qa.secondary_runtime_isolation` | actor-owned sync/isolation |
| `headless_core_qa::contracts::e2ee_strict_qa_uses_typed_causal_checks_after_recipient_device_verification` | `core.qa.strict_waiters` | typed causal waiters |
| `headless_core_qa::contracts::same_user_secondary_device_runtimes_isolate_saved_credentials` | `core.qa.secondary_runtime_isolation` | isolated saved credentials |
| `headless_core_qa::contracts::e2ee_device_verification_labels_distinguish_recipient_second_device` | `core.qa.tracing_and_device_labels` | recipient device labels |
| `headless_core_qa::contracts::focused_send_queue_bootstrap_logs_out_before_ordered_shutdown` | `core.qa.send_queue_secret_and_cleanup` | logout before shutdown |
| `headless_core_qa::contracts::shared_primary_login_always_completes_the_new_identity_gate` | `core.qa.login_gate_lifecycle` | shared identity gate |
| `headless_core_qa::contracts::new_identity_gate_settles_its_bootstrap_confirmation_before_returning` | `core.qa.login_gate_lifecycle` | confirmation settles before return |
| `headless_core_qa::contracts::login_wait_timeout_names_the_session_phase` | `core.qa.login_gate_lifecycle` | phase-bearing login timeout |
| `headless_core_qa::contracts::scenarios_that_must_not_bootstrap_return_before_the_shared_login` | `core.qa.login_gate_lifecycle` | non-bootstrap shared login |
| `headless_core_qa::contracts::run_async_centrally_owns_one_normal_secondary_login` | `core.qa.secondary_lifecycle` | one secondary owner |
| `headless_core_qa::contracts::invites_dm_and_directory_borrow_b_without_owning_its_lifecycle` | `core.qa.secondary_lifecycle` | stages borrow owner |
| `headless_core_qa::contracts::room_space_reuses_and_consumes_the_central_secondary_owner` | `core.qa.secondary_lifecycle` | room/space reuse |
| `headless_core_qa::contracts::normal_secondary_cleanup_paths_use_one_ordered_runtime_shutdown` | `core.qa.secondary_lifecycle` | ordered cleanup |
| `headless_core_qa::contracts::standalone_send_queue_login_requires_primary_recovery_secret` | `core.qa.send_queue_route` | primary recovery secret |
| `headless_core_qa::contracts::participant_login_gate_policy_distinguishes_bootstrap_from_recovery` | `core.qa.send_queue_route` | bootstrap/recovery distinction |
| `headless_core_qa::contracts::strict_e2ee_guard_extracts_each_complete_waiter_body` | `core.qa.strict_waiters` | complete waiter bodies |
| `headless_core_qa::contracts::strict_e2ee_guard_detects_a_rolling_timeout_in_every_inventory_body` | `core.qa.strict_waiters` | no rolling timeout |
| `headless_core_qa::contracts::strict_e2ee_event_waiters_do_not_restart_timeouts_per_event` | `core.qa.strict_waiters` | one deadline per waiter |
| `headless_core_qa::contracts::active_room_thread_refresh_uses_the_exact_causal_waiter` | `core.qa.backup_causal_waiters` | exact thread waiter |
| `headless_core_qa::contracts::e2ee_trust_stage_does_not_overlap_normal_sync_with_manual_sync_once` | `core.qa.no_manual_sync_once` | no SyncOnce overlap |
| `headless_core_qa::contracts::device_cleanup_scenario_has_a_dedicated_remote_first_proof` | `core.qa.device_cleanup` | remote-first proof |
| `headless_core_qa::contracts::encrypted_backup_seed_uses_live_room_discovery_and_exact_causal_waiter` | `core.qa.backup_causal_waiters` | live discovery/waiter |
| `headless_core_qa::contracts::second_device_encrypted_room_resubscribe_uses_exact_causal_waiter` | `core.qa.backup_causal_waiters` | second-device waiter |
| `headless_core_qa::contracts::generic_secondary_timeline_subscribe_uses_exact_causal_waiter` | `core.qa.backup_causal_waiters` | secondary waiter |
| `headless_core_qa::contracts::timeline_stress_uses_event_waiters_not_manual_sync_once` | `core.qa.timeline_stress`, `core.qa.no_manual_sync_once` | event waiters/no SyncOnce |
| `headless_core_qa::contracts::login_wait_uses_dedicated_timeout_for_loaded_local_homeservers` | `core.qa.timeout_and_directory_order` | dedicated timeout |
| `headless_core_qa::contracts::all_directory_stage_runs_before_room_space_operations` | `core.qa.timeout_and_directory_order` | directory ordering |
| `headless_core_qa::contracts::send_queue_fifo_wait_uses_dedicated_reconnect_timeout` | `core.qa.timeout_and_directory_order` | FIFO timeout |
| `headless_core_qa::contracts::send_queue_unsubscribes_timeline_before_runtime_shutdown` | `core.qa.send_queue_secret_and_cleanup` | unsubscribe/shutdown ordering |
| `headless_core_qa::contracts::same_data_dir_reopen_paths_use_ordered_runtime_shutdown` | `core.qa.runtime_reopen_order` | reopen ordering |
| `headless_core_qa::contracts::timeline_stress_backfill_only_advances_current_paginate_request` | `core.qa.timeline_stress` | stale pagination fence |
| `headless_core_qa::contracts::timeline_stress_replay_existing_is_read_only` | `core.qa.timeline_stress` | read-only replay |
| `headless_core_qa::contracts::e2ee_trust_stage_prints_joined_room_restore_scope_token` | `core.qa.restore_scope_and_privacy` | joined-room restore scope |
| `headless_core_qa::contracts::e2ee_trust_stage_reports_second_device_decrypt_token` | `core.qa.restore_scope_and_privacy` | second-device decrypt token |
| `headless_core_qa::contracts::e2ee_trust_stage_reports_multi_user_multi_device_decrypt_token` | `core.qa.restore_scope_and_privacy` | multi-device decrypt token |
| `headless_core_qa::contracts::e2ee_trust_stage_makes_identity_reset_explicitly_opt_in` | `core.qa.restore_scope_and_privacy` | explicit identity reset |
| `headless_core_qa::contracts::core_qa_stdout_does_not_format_matrix_identifiers` | `core.qa.restore_scope_and_privacy` | identifier-free stdout |
| `headless_core_qa::contracts::provisional_self_verification_keeps_primary_normal_sync_running` | `core.qa.provisional_verification` | primary sync retained |
| `headless_core_qa::contracts::unused_manual_second_device_verification_cascade_is_absent` | `core.qa.no_obsolete_verification_cascade` | obsolete cascade absent |
| `headless_core_qa::login_store_contracts::e2ee_login_store_is_a_behavioral_dedicated_route` | `core.qa.login_gate_lifecycle` | source-only route/identity assertions depended on removed helpers |

Integration deletions (all **Removed**, no mixed identities):

| Old identity | Existing rule | Facts / includes |
| --- | --- | --- |
| `runtime_intent_lifecycle::select_room_routing_is_reliable_and_correlated` | `core.integration.select_room_routing` | correlation, awaited route, no lossy send / 2 |
| `runtime_room_list_sync::production_runtime_requires_committed_all_rooms_readiness` | `core.integration.room_list_readiness` | committed readiness, one service, no legacy/probe / 1 |
| `runtime_room_list_sync::production_core_has_no_legacy_or_mode_transition_vocabulary` | `core.integration.no_legacy_mode_vocabulary` | seven forbidden vocabulary predicates / 4 |
| `runtime_timeline::production_timeline_has_no_classic_sync_or_legacy_checkpoint_path` | `core.integration.timeline_no_legacy_checkpoint` | four forbidden timeline tokens / 1 |
| `send_queue_fast::fast_send_queue_lane_hard_bounds_generic_lifecycle_phases` | `core.qa.fast_send_queue_lifecycle` | timeout and lifecycle phase bounds / 1 |
| `send_queue_fast::send_queue_stage_uses_exact_causal_waiter_for_both_subscriptions` | `core.qa.send_queue_causal_waiter` | both subscriptions use waiter / 1 |
| `send_queue_fast::headless_send_queue_diagnostic_contract_counts_forwarded_and_completed_room_sends` | `core.qa.send_queue_diagnostic_counters` | classifier/counter fences / 1 |
| `send_queue_fast::headless_send_queue_diagnostic_contract_wraps_fifo_failure_with_proxy_deltas` | `core.qa.send_queue_proxy_deltas` | retry baselines/deltas / 1 |
| `send_queue_fast::headless_send_queue_diagnostic_contract_arms_before_private_safe_not_sent_failure` | `core.qa.send_queue_private_safe_failure` | private-safe retry diagnostics / 1 |
| `send_queue_fast::fast_send_queue_restored_completion_cannot_finish_from_send_completed_alone` | `core.integration.fast_send_queue_completion` | projection before completion / 1 |

Final slice totals: 57 deleted source-only tests and 14 Rust-source includes;
missing=0, extra=0, vacuous=0, mixed=0. The behavioral identities
`runtime_timeline::production_inventory_covers_code_after_cfg_test_modules`
and `send_queue_fast::fast_send_queue_authoritative_projection_requires_one_exact_event_and_no_transaction`
remain unchanged.

## Large inline module extraction

The post-source-contract inventory contained 72 modules still at or above the 200-line ceiling. A one-shot `/tmp` migration generated `/tmp/issue753-extraction-ledger.tsv`: all 72 sibling bodies initially matched the removed body after one uniform dedent. Before compilation, one moved media test changed only its relative `include_bytes!` fixture path to a manifest-rooted equivalent; the other 71 bodies retained byte-equal dedent proofs. The two crate-root `lib/tests.rs` outputs were moved to Rust's natural `src/tests.rs` path without content changes. The six baseline modules not listed below fell under 200 lines solely because their source-only assertions moved to the checker; strict enforcement confirms they no longer exceed the ceiling.

| Source module | Test module | Old lines | Sibling path | Proof |
| --- | --- | ---: | --- | --- |
| `apps/desktop/src-tauri/src/commands/native_attention.rs` | `tests` | 295 | `apps/desktop/src-tauri/src/commands/native_attention/tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/commands/room.rs` | `tests` | 305 | `apps/desktop/src-tauri/src/commands/room/tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/commands/timeline.rs` | `submission_settlement_tests` | 410 | `apps/desktop/src-tauri/src/commands/timeline/submission_settlement_tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/commands/timeline.rs` | `issue551_moved_tests` | 215 | `apps/desktop/src-tauri/src/commands/timeline/issue551_moved_tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/core_event_forwarder.rs` | `tests` | 1872 | `apps/desktop/src-tauri/src/core_event_forwarder/tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/dto.rs` | `tests` | 1623 | `apps/desktop/src-tauri/src/dto/tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/lib.rs` | `tests` | 338 | `apps/desktop/src-tauri/src/tests.rs` | `dedented-body-byte-equal` |
| `apps/desktop/src-tauri/src/window_state.rs` | `tests` | 434 | `apps/desktop/src-tauri/src/window_state/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account_work.rs` | `tests` | 303 | `crates/koushi-core/src/account_work/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/account_management.rs` | `tests` | 207 | `crates/koushi-core/src/account/account_management/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/local_data_cleanup.rs` | `tests` | 562 | `crates/koushi-core/src/account/local_data_cleanup/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/recovery_backup.rs` | `tests` | 740 | `crates/koushi-core/src/account/recovery_backup/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/runtime_children.rs` | `tests` | 243 | `crates/koushi-core/src/account/runtime_children/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/scheduled_send.rs` | `tests` | 448 | `crates/koushi-core/src/account/scheduled_send/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/session_lifecycle.rs` | `tests` | 2207 | `crates/koushi-core/src/account/session_lifecycle/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/sliding_sync.rs` | `tests` | 324 | `crates/koushi-core/src/account/sliding_sync/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/trust_gate.rs` | `tests` | 1109 | `crates/koushi-core/src/account/trust_gate/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/account/verification.rs` | `tests` | 803 | `crates/koushi-core/src/account/verification/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/command/app.rs` | `tests` | 394 | `crates/koushi-core/src/command/app/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/command/room.rs` | `tests` | 215 | `crates/koushi-core/src/command/room/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/command/timeline.rs` | `tests` | 267 | `crates/koushi-core/src/command/timeline/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/event/timeline.rs` | `tests` | 1017 | `crates/koushi-core/src/event/timeline/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/link_preview.rs` | `tests` | 412 | `crates/koushi-core/src/link_preview/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/live_tail_freshness.rs` | `tests` | 330 | `crates/koushi-core/src/live_tail_freshness/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/media_preparation.rs` | `tests` | 502 | `crates/koushi-core/src/media_preparation/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/mention_candidates.rs` | `tests` | 480 | `crates/koushi-core/src/mention_candidates/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/read_state.rs` | `tests` | 717 | `crates/koushi-core/src/read_state/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/renderable_thumbnail.rs` | `tests` | 218 | `crates/koushi-core/src/renderable_thumbnail/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/actor.rs` | `tests` | 285 | `crates/koushi-core/src/room/actor/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/encryption_debug.rs` | `tests` | 275 | `crates/koushi-core/src/room/encryption_debug/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/list_observer.rs` | `tests` | 1048 | `crates/koushi-core/src/room/list_observer/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/normalization.rs` | `tests` | 546 | `crates/koushi-core/src/room/normalization/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/operations.rs` | `tests` | 257 | `crates/koushi-core/src/room/operations/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/room/space_members.rs` | `tests` | 356 | `crates/koushi-core/src/room/space_members/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/runtime.rs` | `tests` | 2200 | `crates/koushi-core/src/runtime/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/runtime/activity.rs` | `tests` | 757 | `crates/koushi-core/src/runtime/activity/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/runtime/connection.rs` | `tests` | 524 | `crates/koushi-core/src/runtime/connection/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/runtime/navigation.rs` | `tests` | 296 | `crates/koushi-core/src/runtime/navigation/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/runtime/reducer_support.rs` | `tests` | 274 | `crates/koushi-core/src/runtime/reducer_support/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/search_crawler.rs` | `tests` | 378 | `crates/koushi-core/src/search_crawler/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/search.rs` | `tests` | 588 | `crates/koushi-core/src/search/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/store/composer_drafts.rs` | `tests` | 307 | `crates/koushi-core/src/store/composer_drafts/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/store/composer_drafts.rs` | `store_tests` | 462 | `crates/koushi-core/src/store/composer_drafts/store_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/store/credential_backend.rs` | `tests` | 660 | `crates/koushi-core/src/store/credential_backend/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/store/navigation.rs` | `tests` | 233 | `crates/koushi-core/src/store/navigation/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/store/read_state.rs` | `tests` | 382 | `crates/koushi-core/src/store/read_state/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/sync.rs` | `tests` | 296 | `crates/koushi-core/src/sync/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/threads_list.rs` | `tests` | 1285 | `crates/koushi-core/src/threads_list/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/actor.rs` | `tests` | 280 | `crates/koushi-core/src/timeline/actor/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/composer.rs` | `tests` | 335 | `crates/koushi-core/src/timeline/composer/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/diagnostics.rs` | `tests` | 995 | `crates/koushi-core/src/timeline/diagnostics/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/display_projection.rs` | `tests` | 1284 | `crates/koushi-core/src/timeline/display_projection/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/gap_repair.rs` | `tests` | 2336 | `crates/koushi-core/src/timeline/gap_repair/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/item_projection.rs` | `tests` | 1268 | `crates/koushi-core/src/timeline/item_projection/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/navigation.rs` | `tests` | 2669 | `crates/koushi-core/src/timeline/navigation/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/outbound_send.rs` | `tests` | 2740 | `crates/koushi-core/src/timeline/outbound_send/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/read_state.rs` | `tests` | 2863 | `crates/koushi-core/src/timeline/read_state/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/relay.rs` | `tests` | 634 | `crates/koushi-core/src/timeline/relay/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/residency.rs` | `tests` | 300 | `crates/koushi-core/src/timeline/residency/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/room_key_recovery.rs` | `tests` | 1150 | `crates/koushi-core/src/timeline/room_key_recovery/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-core/src/timeline/thread_projection.rs` | `tests` | 1618 | `crates/koushi-core/src/timeline/thread_projection/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-diagnostics/src/lib.rs` | `tests` | 388 | `crates/koushi-diagnostics/src/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/e2ee.rs` | `device_cleanup_tests` | 236 | `crates/koushi-sdk/src/e2ee/device_cleanup_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/e2ee.rs` | `secure_backup_inspection_tests` | 285 | `crates/koushi-sdk/src/e2ee/secure_backup_inspection_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/e2ee.rs` | `e2ee_trust_tests` | 821 | `crates/koushi-sdk/src/e2ee/e2ee_trust_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/e2ee.rs` | `current_session_status_tests` | 316 | `crates/koushi-sdk/src/e2ee/current_session_status_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/e2ee.rs` | `initial_share_diagnostics_tests` | 309 | `crates/koushi-sdk/src/e2ee/initial_share_diagnostics_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/room_operations.rs` | `tests` | 497 | `crates/koushi-sdk/src/room_operations/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/room_projection.rs` | `space_member_projection_tests` | 273 | `crates/koushi-sdk/src/room_projection/space_member_projection_tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/room_projection.rs` | `tests` | 678 | `crates/koushi-sdk/src/room_projection/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-sdk/src/sync.rs` | `tests` | 453 | `crates/koushi-sdk/src/sync/tests.rs` | `dedented-body-byte-equal` |
| `crates/koushi-state/src/reducer/mod.rs` | `tests` | 1606 | `crates/koushi-state/src/reducer/tests.rs` | `dedented-body-byte-equal` |

Baseline modules no longer requiring extraction after source-contract removal:
- `apps/desktop/src-tauri/src/commands/diagnostics.rs` — `snapshot_tests`
- `apps/desktop/src-tauri/src/commands/e2ee.rs` — `tests`
- `apps/desktop/src-tauri/src/commands/navigation.rs` — `tests`
- `crates/koushi-core/src/account/routing.rs` — `tests`
- `crates/koushi-core/src/timeline/manager.rs` — `tests`
- `crates/koushi-core/src/timeline/media.rs` — `tests`

Extraction summary: moved modules = 72; new sibling files = 72; exact dedent proofs = 71; approved fixture-path-only normalization = 1; strict inline modules >=200 after extraction = 0.

## Final collection audit

- Workspace baseline: 2564 test identities.
- Workspace after migration/extraction: 2341 identities.
- Removed: 223 source-only identities, all present in the mapping sections above.
- Added/renamed behavioral identities: 0.
- Core QA baseline: 135 identities; final: 88.
- Core QA removed: 47 source-only identities, all mapped; added/renamed behavioral identities: 0.
- Strict checker: zero Rust-source `include_str!`, exactly four approved non-Rust embeds, zero inline cfg-test modules at or above 200 lines, and zero source-rule failures.
- Full workspace tests and the Core QA binary tests pass after extraction.
