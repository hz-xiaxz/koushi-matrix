# Issue #551 headless-core-QA design inventory

This appendix pins the item and test ownership required by Wave 2A of the canonical Issue #551 plan. The authoritative immutable source is `crates/koushi-core/src/bin/headless-core-qa.rs` at merge base `8cfef95ff207ecc2aafc18584c6e270cdc3c1ca4` (23,366 lines, SHA-256 `be418fd3bcf250897a01e5aedf552f3881066ceea50f3b61ff175cea04b6cdcf`). Line numbers below refer only to that blob; extraction uses complete parsed top-level items with leading attributes rather than mutable line slices.

## Cross-owner production item pins

| Owner | Complete item cluster |
| --- | --- |
| `registry.rs` | `ENV_*`, device/timeout/stress/cache constants; `QaScenario`, `QaStage`, `QaConfig`, `TimelineStressConfig`; scenario parsing/preflight/stage/token/report functions; `env_required`, `bounded_usize_env`, `env_flag_enabled`, `parse_env_flag`. |
| `event_wait.rs` | `QaEventFuture`, `QaEventSource`, `QaSnapshotEventSource`, `QaEventDeadline`, `PairedEventWaitError`, paired wait; `SendFlowOutcome`, `SendQueueLocalEcho`, `SendFlowWaiter`, `BodyWaitObserver`; `wait_for_send_flow_completion`, `wait_for_send_flow_completion_with_timeout`, `send_text_expect_local_echo`, `subscribe_timeline_for_qa`, `wait_for_initial_items`, `wait_for_initial_items_with_timeout`, `wait_for_item_with_body_or_decryption_failure`, `wait_for_bodies_and_pagination_settle`, `timeline_item_body_matches`, and generic session/sync/logout/operation-failure deadline waiters. `assert_no_decryption_failure_items` is excluded and remains identity-owned. |
| `participants.rs` | `qa_data_dir`; `QaParticipantLoginGate`, `QaParticipantLoginOutcome`, owned participant/session phase types; participant login/bootstrap/recovery/identity-gate construction and verification checkpoint helpers. |
| `cleanup.rs` | `QaE2eeLogoutBarrier`, `QaOwnedE2eeCleanupOperations`, `QaCoreOwnedE2eeCleanupOperations`; ordered stop/logout/authoritative-settle/drop/shutdown functions; all participant/full-flow/data-directory cleanup and best-effort aggregation. |
| `diagnostics.rs` | private-safe timeout/category/count formatting; TCP proxy server/handle/request kind/action/message action/canned-page items; forwarding/counter/order helpers. It imports no scenario module. |
| `scenarios/identity.rs` | gate restore/no-proof/negative, provisional device cleanup, session status, credential health, native attention, encryption debug, E2EE trust, `assert_no_decryption_failure_items`, and identity-only predicates/waiters. |
| `scenarios/rooms.rs` | invites/DM, room/space, directory, room management, room people/mentions/pins, and their owner-only predicates/waiters. |
| `scenarios/timeline.rs` | timeline/reply/thread/reconnect/stress/activity/composer/media/live signals/scheduled send/send queue/cache restore/link preview; send-queue-specific observers including `observe_send_queue_retry_item_state`, `wait_for_send_completions_in_order`, `wait_for_cancelled_or_removed_send`, `unsubscribe_timeline_for_qa`, and `assert_zero_display_projection_reset_fallback_delta`. It consumes generic send/body waits from `event_wait.rs`; it does not export them. |
| `scenarios/search.rs` | edit/redact/search crawler and their owner-only query/index waiters. |
| `orchestrator.rs` | `run_async` unchanged as the only cross-family stage composition; no scenario payload helper and no token mapping. |

**Edge rule:** scenario files never import sibling scenario files. All cross-family send/body/deadline helpers named above live in `event_wait.rs`; lifecycle items live in `cleanup.rs`; construction lives in `participants.rs`. Any unlisted helper with callers in more than one scenario owner follows those same three categories and must be added to this table and re-reviewed before implementation, not decided during integration.

## Source-characterization guard pins

The base contains 53 production-source reads across 49 test functions (49 `include_str!`/manifest-path test sites, with four tests using the explicit manifest path). All 49 tests move to `contracts.rs`; none remains owner-local. `contracts::production_source()` concatenates root, registry, event-wait, participants, cleanup, diagnostics, orchestrator, and all four scenario production files after trimming each at its owner-local `#[cfg(test)]` boundary. Contract test bodies are therefore absent from the searched text. Tests that require one function body slice use start/end markers from the same final owner file; cross-owner absence/order checks use the concatenated production source.

The four external `crates/koushi-core/tests/send_queue_fast.rs` guards are pinned as follows: the guards beginning at base lines 1588, 1661, and 1700 read `headless_core_qa/scenarios/timeline.rs`; the classifier/proxy counter guard beginning at 1606 reads `headless_core_qa/diagnostics.rs`. The existing guard near 1533 that checks generic fast-phase ownership continues to read its production owner selected by its asserted function after extraction. `releaseScripts.test.ts` replaces the single root entry with the explicit 11-file production list (root, registry, event_wait, participants, cleanup, diagnostics, orchestrator, identity, rooms, timeline, search), excluding `contracts.rs` and owner test files.

### All source-characterization contract tests (49)

- `active_reconnect_uses_encryption_gate_before_timeline_work` (base line 18189)
- `invite_timeout_uses_private_safe_observer_diagnostic_summary` (base line 18595)
- `production_qa_never_overlaps_actor_owned_sync_with_manual_sync_once` (base line 18613)
- `owner_driven_e2ee_body_waiter_keeps_the_extended_deadline` (base line 18631)
- `unverified_peer_refreshes_device_keys_before_behavioral_checkpoints` (base line 18647)
- `e2ee_key_delivery_preestablishes_invite_before_optional_b_login` (base line 18699)
- `headless_qa_binary_initializes_rust_log_tracing` (base line 19971)
- `e2ee_strict_qa_keeps_actor_owned_sync_running_for_multi_device_send` (base line 19985)
- `e2ee_strict_qa_uses_typed_causal_checks_after_recipient_device_verification` (base line 20002)
- `e2ee_device_verification_labels_distinguish_recipient_second_device` (base line 20020)
- `send_queue_display_projection_fallback_gate_requires_zero_counter_delta` (base line 20206)
- `send_queue_alone_uses_the_focused_early_route` (base line 20227)
- `focused_send_queue_bootstrap_logs_out_before_ordered_shutdown` (base line 20286)
- `shared_primary_login_always_completes_the_new_identity_gate` (base line 20331)
- `new_identity_gate_settles_its_bootstrap_confirmation_before_returning` (base line 20355)
- `login_wait_timeout_names_the_session_phase` (base line 20390)
- `scenarios_that_must_not_bootstrap_return_before_the_shared_login` (base line 20412)
- `run_async_centrally_owns_one_normal_secondary_login` (base line 20469)
- `invites_dm_and_directory_borrow_b_without_owning_its_lifecycle` (base line 20500)
- `room_space_reuses_and_consumes_the_central_secondary_owner` (base line 20538)
- `normal_secondary_cleanup_paths_use_one_ordered_runtime_shutdown` (base line 20564)
- `all_flow_retains_the_primary_recovery_secret_for_its_send_queue_stage` (base line 20590)
- `standalone_send_queue_login_requires_primary_recovery_secret` (base line 20620)
- `participant_login_gate_policy_distinguishes_bootstrap_from_recovery` (base line 20640)
- `strict_e2ee_guard_extracts_each_complete_waiter_body` (base line 21974)
- `strict_e2ee_guard_detects_a_rolling_timeout_in_every_inventory_body` (base line 21991)
- `strict_e2ee_event_waiters_do_not_restart_timeouts_per_event` (base line 22014)
- `active_room_thread_refresh_uses_the_exact_causal_waiter` (base line 22028)
- `e2ee_trust_stage_does_not_overlap_normal_sync_with_manual_sync_once` (base line 22042)
- `device_cleanup_scenario_has_a_dedicated_remote_first_proof` (base line 22062)
- `encrypted_backup_seed_uses_live_room_discovery_and_exact_causal_waiter` (base line 22094)
- `second_device_encrypted_room_resubscribe_uses_exact_causal_waiter` (base line 22114)
- `generic_secondary_timeline_subscribe_uses_exact_causal_waiter` (base line 22128)
- `timeline_stress_uses_event_waiters_not_manual_sync_once` (base line 22240)
- `login_wait_uses_dedicated_timeout_for_loaded_local_homeservers` (base line 22262)
- `all_directory_stage_runs_before_room_space_operations` (base line 22296)
- `send_queue_fifo_wait_uses_dedicated_reconnect_timeout` (base line 22322)
- `send_queue_unsubscribes_timeline_before_runtime_shutdown` (base line 22349)
- `same_data_dir_reopen_paths_use_ordered_runtime_shutdown` (base line 22398)
- `timeline_stress_backfill_only_advances_current_paginate_request` (base line 22470)
- `timeline_stress_replay_existing_is_read_only` (base line 22516)
- `e2ee_trust_stage_prints_joined_room_restore_scope_token` (base line 22720)
- `e2ee_trust_stage_reports_second_device_decrypt_token` (base line 22729)
- `e2ee_trust_stage_reports_multi_user_multi_device_decrypt_token` (base line 22737)
- `e2ee_trust_stage_makes_identity_reset_explicitly_opt_in` (base line 22748)
- `core_qa_stdout_does_not_format_matrix_identifiers` (base line 22774)
- `provisional_self_verification_keeps_primary_normal_sync_running` (base line 22871)
- `incoming_verification_waiter_rejects_stopped_receiver_sync_at_entry` (base line 22913)
- `unused_manual_second_device_verification_cascade_is_absent` (base line 22975)

## Complete 129-test owner manifest

The counts are normative and sum to 129. A test name may occur exactly once after extraction. The 49 source-characterization tests above are repeated under `contracts.rs` here so this section is self-contained.

### `registry.rs` — 11 tests

- `parses_all_scenarios_from_env_value_including_directory` (base line 19012)
- `rejects_unknown_scenario_names` (base line 19117)
- `supported_scenarios_are_allowed_by_preflight` (base line 19125)
- `session_status_scenario_runs_after_login_and_reports_only_safe_tokens` (base line 19157)
- `thread_is_allowed_by_preflight` (base line 19173)
- `all_core_qa_scenarios_suppress_matrix_identifiers` (base line 19178)
- `staged_scenarios_stop_after_their_requested_stage` (base line 20037)
- `implemented_final_tokens_include_thread` (base line 22628)
- `parse_env_flag_accepts_only_explicit_boolean_values` (base line 22757)
- `final_tokens_follow_the_requested_scenario_including_composer` (base line 22999)
- `implemented_final_tokens_include_safety` (base line 23275)

### `diagnostics.rs` — 4 tests

- `trust_admission_timeout_summary_is_allowlisted_and_private_safe` (base line 18445)
- `invite_timeout_diagnostic_summary_is_allowlisted_and_private_safe` (base line 18500)
- `send_queue_proxy_forces_connection_close_per_request` (base line 22142)
- `live_tail_proxy_enforces_tokenless_refresh_and_exact_continuation_requests` (base line 22160)

### `event_wait.rs` — 20 tests

- `diff_item_visitor_scans_set_and_reset_items` (base line 19611)
- `body_wait_observer_tolerates_transient_decryption_failure_before_expected_body` (base line 19643)
- `find_timeline_item_with_body_finds_thread_reply_in_one_batch` (base line 19683)
- `find_timeline_item_with_body_returns_none_when_missing` (base line 19726)
- `send_flow_waiter_accepts_send_completed_before_local_echo` (base line 19764)
- `send_flow_waiter_status_reports_local_echo_send_state` (base line 19840)
- `send_flow_waiter_errors_when_local_echo_becomes_not_sent` (base line 19906)
- `initial_items_wait_requires_exact_subscribe_cause_even_for_same_key_replays` (base line 20931)
- `withheld_projection_wait_accepts_decryption_failure_from_late_items_updated` (base line 21043)
- `withheld_projection_wait_reports_private_safe_missing_category_at_deadline` (base line 21073)
- `withheld_projection_wait_rejects_plaintext_without_exposing_it` (base line 21099)
- `paired_verification_wait_wakes_from_either_event_source` (base line 21127)
- `paired_verification_wait_uses_one_absolute_deadline` (base line 21147)
- `login_wait_observes_ready_snapshot_once_at_deadline_without_a_broadcast` (base line 21348)
- `login_wait_without_event_or_ready_snapshot_still_times_out` (base line 21387)
- `session_restored_account_mismatch_is_private_safe` (base line 21418)
- `logout_and_operation_failed_deadlines_survive_unrelated_event_starvation` (base line 21725)
- `initial_items_wait_deadline_is_not_extended_by_continuous_unrelated_events` (base line 21840)
- `initial_items_wait_skips_fresh_wrong_cause_then_accepts_exact_replay_cause` (base line 21877)
- `initial_items_timeout_reports_only_private_safe_causal_category_counts` (base line 21922)

### `participants.rs` — 4 tests

- `stale_gate_failure_is_not_attributed_to_a_fresh_sas_flow` (base line 18960)
- `incoming_waiter_ignores_the_previous_terminal_flow` (base line 18986)
- `normal_secondary_participant_policy_covers_only_shared_b_stages` (base line 20437)
- `receiver_device_checkpoint_holds_start_once_until_ack_and_skips_it_on_failure` (base line 22829)

### `cleanup.rs` — 11 tests

- `owned_e2ee_recipient_cleanup_runs_after_post_login_stage_failure` (base line 20662)
- `borrowed_e2ee_stage_failure_runs_outer_caller_cleanup_path` (base line 20688)
- `owned_e2ee_cleanup_orders_each_ownership_phase` (base line 20793)
- `borrowed_e2ee_recipient_is_not_cleaned_by_the_inner_stage` (base line 20854)
- `e2ee_multi_device_cleanup_attempts_every_owned_participant_after_one_failure` (base line 20879)
- `logged_out_waiter_requires_event_and_signed_out_snapshot_in_either_order` (base line 21447)
- `logout_waiters_observe_final_signed_out_snapshot_without_another_broadcast` (base line 21494)
- `logout_waiters_observe_final_signed_out_snapshot_after_lag_or_close` (base line 21542)
- `logged_out_waiter_keeps_wrong_account_and_failure_terminal_and_private_safe` (base line 21576)
- `operation_failed_signed_out_waiter_requires_both_signals_in_either_order` (base line 21626)
- `operation_failed_signed_out_deadline_survives_unrelated_event_starvation` (base line 21700)

### `scenarios/identity.rs` — 1 tests

- `e2ee_trust_qa_uses_authenticated_provisional_session_info` (base line 22792)

### `scenarios/rooms.rs` — 2 tests

- `room_management_scenario_runs_after_room_space_and_reports_private_tokens` (base line 22578)
- `room_management_forbidden_predicate_requires_matching_failed_moderation_state` (base line 22601)

### `scenarios/timeline.rs` — 27 tests

- `reconnect_initial_projection_rejects_missing_newest_body` (base line 18242)
- `reconnect_initial_projection_rejects_oldest_present_before_page` (base line 18257)
- `reconnect_initial_projection_requires_mandatory_pagination` (base line 18272)
- `reconnect_pagination_requires_paginating_before_terminal` (base line 18286)
- `reconnect_projection_applies_destructive_diffs_exactly_and_rejects_duplicates` (base line 18316)
- `reconnect_terminal_before_paginating_is_rejected` (base line 18371)
- `reconnect_terminal_can_precede_the_final_diff` (base line 18390)
- `reconnect_projection_rejects_undecipherable_items` (base line 18432)
- `visible_gap_selector_prefers_internal_gap_and_returns_nearest_event_bounds` (base line 18742)
- `visible_gap_selector_chooses_newest_internal_gap_from_reversed_positions` (base line 18790)
- `visible_gap_selector_chooses_newest_top_row_gap_without_event_bounds` (base line 18834)
- `visible_gap_selector_rejects_unbracketed_non_top_gaps_privately` (base line 18864)
- `visible_gap_capture_requires_a_post_body_projection` (base line 18898)
- `finds_timeline_item_in_initial_items_by_body_substring` (base line 19215)
- `thread_reply_missing_from_initial_items_requires_paginate_backfill` (base line 19291)
- `thread_reply_present_in_initial_items_does_not_require_backfill` (base line 19332)
- `thread_reply_stops_repagination_after_end_reached` (base line 19373)
- `thread_summary_helper_requires_root_item_with_reply_count` (base line 19420)
- `room_thread_assertion_requires_canonical_reply_and_root_summary` (base line 19455)
- `room_thread_summary_observer_waits_for_late_summary_diff` (base line 19515)
- `room_thread_summary_observer_accepts_canonical_thread_reply` (base line 19551)
- `thread_qa_reports_canonical_reply_contract` (base line 19566)
- `thread_relation_helper_requires_thread_root_and_validates_optional_reply_metadata` (base line 19574)
- `send_queue_scenario_skips_generic_fixture_stages_and_reports_private_tokens` (base line 20176)
- `canned_live_tail_messages_page_reproduces_a_gap_before_the_known_latest_event` (base line 22203)
- `timeline_stress_blank_row_detection_rejects_empty_formatted_body` (base line 22494)
- `scheduled_send_scenario_runs_after_timeline_and_reports_private_tokens` (base line 22544)

### `scenarios/search.rs` — 0 tests

- No baseline unit test is owner-local; production coverage is exercised by the centralized registry/contracts and disposable-server lane.

### `contracts.rs` — 49 tests

- `active_reconnect_uses_encryption_gate_before_timeline_work` (base line 18189)
- `invite_timeout_uses_private_safe_observer_diagnostic_summary` (base line 18595)
- `production_qa_never_overlaps_actor_owned_sync_with_manual_sync_once` (base line 18613)
- `owner_driven_e2ee_body_waiter_keeps_the_extended_deadline` (base line 18631)
- `unverified_peer_refreshes_device_keys_before_behavioral_checkpoints` (base line 18647)
- `e2ee_key_delivery_preestablishes_invite_before_optional_b_login` (base line 18699)
- `headless_qa_binary_initializes_rust_log_tracing` (base line 19971)
- `e2ee_strict_qa_keeps_actor_owned_sync_running_for_multi_device_send` (base line 19985)
- `e2ee_strict_qa_uses_typed_causal_checks_after_recipient_device_verification` (base line 20002)
- `e2ee_device_verification_labels_distinguish_recipient_second_device` (base line 20020)
- `send_queue_display_projection_fallback_gate_requires_zero_counter_delta` (base line 20206)
- `send_queue_alone_uses_the_focused_early_route` (base line 20227)
- `focused_send_queue_bootstrap_logs_out_before_ordered_shutdown` (base line 20286)
- `shared_primary_login_always_completes_the_new_identity_gate` (base line 20331)
- `new_identity_gate_settles_its_bootstrap_confirmation_before_returning` (base line 20355)
- `login_wait_timeout_names_the_session_phase` (base line 20390)
- `scenarios_that_must_not_bootstrap_return_before_the_shared_login` (base line 20412)
- `run_async_centrally_owns_one_normal_secondary_login` (base line 20469)
- `invites_dm_and_directory_borrow_b_without_owning_its_lifecycle` (base line 20500)
- `room_space_reuses_and_consumes_the_central_secondary_owner` (base line 20538)
- `normal_secondary_cleanup_paths_use_one_ordered_runtime_shutdown` (base line 20564)
- `all_flow_retains_the_primary_recovery_secret_for_its_send_queue_stage` (base line 20590)
- `standalone_send_queue_login_requires_primary_recovery_secret` (base line 20620)
- `participant_login_gate_policy_distinguishes_bootstrap_from_recovery` (base line 20640)
- `strict_e2ee_guard_extracts_each_complete_waiter_body` (base line 21974)
- `strict_e2ee_guard_detects_a_rolling_timeout_in_every_inventory_body` (base line 21991)
- `strict_e2ee_event_waiters_do_not_restart_timeouts_per_event` (base line 22014)
- `active_room_thread_refresh_uses_the_exact_causal_waiter` (base line 22028)
- `e2ee_trust_stage_does_not_overlap_normal_sync_with_manual_sync_once` (base line 22042)
- `device_cleanup_scenario_has_a_dedicated_remote_first_proof` (base line 22062)
- `encrypted_backup_seed_uses_live_room_discovery_and_exact_causal_waiter` (base line 22094)
- `second_device_encrypted_room_resubscribe_uses_exact_causal_waiter` (base line 22114)
- `generic_secondary_timeline_subscribe_uses_exact_causal_waiter` (base line 22128)
- `timeline_stress_uses_event_waiters_not_manual_sync_once` (base line 22240)
- `login_wait_uses_dedicated_timeout_for_loaded_local_homeservers` (base line 22262)
- `all_directory_stage_runs_before_room_space_operations` (base line 22296)
- `send_queue_fifo_wait_uses_dedicated_reconnect_timeout` (base line 22322)
- `send_queue_unsubscribes_timeline_before_runtime_shutdown` (base line 22349)
- `same_data_dir_reopen_paths_use_ordered_runtime_shutdown` (base line 22398)
- `timeline_stress_backfill_only_advances_current_paginate_request` (base line 22470)
- `timeline_stress_replay_existing_is_read_only` (base line 22516)
- `e2ee_trust_stage_prints_joined_room_restore_scope_token` (base line 22720)
- `e2ee_trust_stage_reports_second_device_decrypt_token` (base line 22729)
- `e2ee_trust_stage_reports_multi_user_multi_device_decrypt_token` (base line 22737)
- `e2ee_trust_stage_makes_identity_reset_explicitly_opt_in` (base line 22748)
- `core_qa_stdout_does_not_format_matrix_identifiers` (base line 22774)
- `provisional_self_verification_keeps_primary_normal_sync_running` (base line 22871)
- `incoming_verification_waiter_rejects_stopped_receiver_sync_at_entry` (base line 22913)
- `unused_manual_second_device_verification_cascade_is_absent` (base line 22975)

## Deterministic acceptance

- Parse the immutable blob and current tree into complete top-level production items and test functions; compare the exact 129-name manifest and reject missing/duplicate/unknown names.
- Compare normalized item bodies after removing only approved visibility/module qualification differences; preserve leading attributes, diagnostic strings, token order, stage-call order, cleanup order, and proxy counter/write/copy order.
- Require the production module graph and every `pub(super)` edge to match the owner table; reject sibling-scenario edges, `pub`/`pub(crate)` growth, glob imports, re-exports, wrappers, TODOs, and production copies in root/tests.
- Run every source-characterization contract against production-only text and mutation-check representative guards by inserting the forbidden literal only into `contracts.rs`; the test must still fail.
