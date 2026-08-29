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
| `send_backup_policy.rs:all_session_constructors_leave_the_per_send_backup_fence_disabled` | 12 / disabled backup-fence count | `sdk.sessions.no_per_send_backup_fence` | Removed |
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
