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
