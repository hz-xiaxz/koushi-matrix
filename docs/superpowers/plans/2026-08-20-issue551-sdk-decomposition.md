# Issue #551: `koushi-sdk` feature-seam decomposition

## Status

- Design review: `reviewer-flash` (read-only, cross-model), final verdict `Correct-to-implement` after ownership/API exactness amendments.
- Implementation: integrated from the immutable baseline; focused SDK gates and bidirectional AST/token exactness audit are green.
- Full-diff review: `reviewer-flash` (read-only, cross-model) reviewed `19fa81a2..66881f2` and recorded `Correct-to-merge`; no findings.
- Delivery gate: all required local repository gates are green; PR CI and merge remain pending.

## Objective

Replace the 16,198-line `crates/koushi-sdk/src/lib.rs` with a small private-module façade and explicit flat re-exports. Move existing items, tests, attributes, cfg gates, docs, strings, task ownership, cleanup, and call order without changing behavior or public API.

This is a source-ownership change only. It does not redesign Matrix semantics, state machines, retries, subscriptions, verification, room-key behavior, sync, DTOs, errors, reports, or QA.

## Immutable baseline

- Commit: `19fa81a2ccc656996d6d3440d3a205730ddacae3`
- Source: `crates/koushi-sdk/src/lib.rs`
- `wc -l`: 16,198 newline-terminated lines; 16,199 content lines including the final non-newline line
- Bytes: 591,305
- SHA-256: `01918c451f766f960d77cc0d4ff870b6873a90e85d55a753b720ebd6f5aaef49`
- Default lib baseline: 143 passed
- `test-hooks` lib baseline: 143 passed
- `--all-targets --all-features` check: green (vendor warnings only)

Every extraction and body comparison uses the immutable baseline blob, never a line-shifted intermediate file.

## One ownership-area PR decision

The governing plan normally uses one independently mergeable seam per PR. For this crate, the SDK adapter is one ownership area and will be delivered as one atomic PR with independently verified internal feature waves.

Rationale:

1. All public consumers use the same flat `koushi_sdk::Item` surface.
2. `MatrixClientSession` inherent impl blocks and DTOs are densely shared across the proposed private modules.
3. Separate PRs would repeatedly add temporary root routing/re-exports and broaden sibling visibility, then remove them later.
4. One integration owner can replace the root once, preserve one exhaustive API manifest, and compare the complete public/cfg/test inventory once.
5. This follows the accepted StoreActor and Tauri-adapter ownership-area batching precedent while staying within one crate/layer.

The exception does not permit mixed product behavior. If a move exposes a defect or requires a semantic change, stop and handle it in a separate verify-first issue/PR.

## Target layout

All new modules are private. No `pub mod`, `mod.rs`, barrel, glob import, wrapper service, one-implementation trait, compatibility alias, or public feature namespace.

```text
crates/koushi-sdk/src/
├── lib.rs
├── sliding_sync_discovery.rs   # existing; unchanged
├── client_session.rs
├── auth.rs
├── profile.rs
├── sync.rs
├── timeline.rs
├── search.rs
├── room_operations.rs
├── room_projection.rs
├── e2ee.rs
├── qa_reports.rs
└── test_source.rs              # cfg(test), source-contract helper only
```

External source-contract tests share `crates/koushi-sdk/tests/support/mod.rs`; because it is nested below `tests/`, it is support code rather than a standalone integration-test target.

`lib.rs` retains only:

- private `mod` declarations;
- the existing `sliding_sync_discovery` module;
- explicit, exhaustive `pub use module::{...};` declarations preserving every baseline flat public path;
- the existing `pub use koushi_state::E2eeRecoveryState`;
- one `#[cfg(test)] mod test_source;` declaration for brace-aware source-contract support.

No production function, type implementation, test body, lifecycle owner, diagnostic registry, or behavioral constant remains in the root.

## Ownership map

Line references are baseline guides; item boundaries, attached attributes/docs, and brace-aware extraction are authoritative.

### `client_session.rs`

Owns client/store construction, `MatrixClientSession`, persistable session data, event-cache enablement, restore, logout, and store-close barriers.

Baseline clusters: 4056–4151, 4215–4220, session impl portions 4776–4794 and 4921–4944, `impl fmt::Debug for MatrixClientSession` at 5268–5278, 5631–5827, 7123–7133, 8247–8352, 8708–8723, 11227–11236, and their focused tests.

### `auth.rs`

Owns homeserver/login discovery, password/OIDC login and account-management capability/password/deactivation contracts.

Baseline clusters: 723–838, 2360–2472, auth-owned declarations within 5828–5954, 6874–6896, 6986–7060, 7146–7333, 13240–13365, and tests.

### `profile.rs`

Owns own-profile, aliases, ignore/unignore, profile/reporting operations, errors, mappings, and tests.

Baseline clusters: 6322–6342 (`MatrixOwnProfile` and `MatrixLocalUserAliases`), 6673–6768, 8724–8908, 13054–13174, and corresponding tests in the mixed root test module. `MatrixUserProfile` is excluded because it is a room/member projection DTO.

### `sync.rs`

Owns invite-list probing, sync-loop control, one-shot/continuous sync, provisional encryption-sync permit ownership, and tests. Existing `sliding_sync_discovery.rs` stays separate and unchanged.

Baseline clusters: 4221–4227, 6868–6873, 6897–6985, 7061–7069, 11215–11226, 11237–11306, 2061–2122, and sync tests in 13367–15444.

### `timeline.rs`

Owns timeline continuity/gap/checkpoint/live-tail types, timeline subscription and pagination handles, item/diff projection, send/edit/redact entry points, cancellation, and tests. No speculative media module is introduced because the baseline has no independent media implementation seam.

Baseline clusters: 4228–4775, session impl portions 4795–4920, 6769–6867, 7081–7109, `send_text_message`/`map_room_send_result` at 8910–8934, 10751–10791, 11138–11214, 12229–12294, 13217–13226, and focused tests. Room capability/settings functions beginning at 8936 belong to room operations; attached item boundaries, not numeric adjacency, decide the move.

### `search.rs`

Owns encrypted search-index configuration, search DTOs/errors/scopes, blocking and async search APIs, mapping, and tests.

Baseline clusters: 4152–4214, 6027–6050, 7110–7122, 10792–10887, and focused tests.

### `room_projection.rs`

Owns room-list/space/activity/invite/member projection DTOs, room-list snapshots/diffs, normalization, direct-account-data mapping, attention projection, SDK-to-adapter mapping, and tests.

Baseline clusters: 6051–6058, 6127–6321, `MatrixUserProfile` with its attached derive at 6343–6363, 6435–6613, `room_list_snapshot_blocking` at 7070–7080, 10888–11036, 11050–11070, 11307–12173, 12295–13053 excluding profile helpers 13054–13174, `matrix_parent_space_ids` and `matrix_space_child_room_ids` at 13175–13216, and focused tests.

### `room_operations.rs`

Owns room/space/directory creation and management, invite/DM/join/leave/forget, tags, pins, read state, notification/settings/moderation operations and their errors/tests.

Baseline clusters: 5955–6026, 6364–6434, 6614–6672, 8936–8959, 9463–10750 excluding separately owned search/timeline items, the public tail item `room_is_joined` at 16192–16199, and focused tests.

### `e2ee.rs`

Owns trust, verification observers, secure backup, key import/export, device cleanup, encryption diagnostics, recovery, outbound-key controls, room-key diagnostic registry, and all related tests.

Baseline clusters: 14–673, 839–4055 excluding specifically reassigned client/sync tests, session impl portions 4945–5267, 5279–5630, 7134–7145, 7334–8246, 8353–8707, 8960–9462, `map_sdk_recovery_state` at 13232–13237, and E2EE tests in 13367–16191. The file-tail `room_is_joined` is explicitly excluded and owned by room operations.

The diagnostic reset/dispatch registry remains cohesive and centralized in this module.

### `qa_reports.rs`

Owns only pure private-data-free report DTOs, `Display` implementations, and builders.

Baseline clusters: 6059–6126, 11037–11049, 11071–11137. `room_attention_summary_from_counts` remains with product room projection.

## Dependency order

Integration order:

1. client/session + search
2. auth + profile
3. sync + timeline
4. room projection + room operations
5. E2EE
6. QA reports
7. remove temporary `MatrixOwnProfile`/`MatrixLocalUserAliases` copies from `room_projection.rs` after `profile.rs` owns them, before any root re-export wiring
8. root façade/API manifest and exactness audit

This order minimizes temporary sibling visibility. It is not permission to add forwarding wrappers.

## Parallel implementation

Mechanical extraction is delegated to Luna/low write-capable workers on isolated worktrees or disjoint destination files. Workers receive the immutable baseline and may create only their assigned new files. They must not edit `lib.rs`, shared scripts, Cargo manifests, or another worker’s destination.

Maximum four concurrent workers:

- Worker A: `client_session.rs`, `search.rs`
- Worker B: `auth.rs`, `profile.rs`
- Worker C: `sync.rs`, `timeline.rs`
- Worker D: `room_projection.rs`, `room_operations.rs`
- Follow-up isolated workers: `e2ee.rs`, `qa_reports.rs`

Workers copy exact items/tests with attached attributes/docs. They may add direct imports and the minimum `pub(super)` visibility required by a proven sibling caller. They do not run full builds against the still-monolithic root. Each reports moved item/test inventories, body hashes, and ambiguity; ambiguity stops that worker.

One integration owner alone edits `lib.rs`, resolves imports/visibility, and qualifies sibling calls through their owning private module. It must not route sibling calls through root re-exports or duplicate helpers.

## Visibility and API rules

- Existing public items remain `pub` and are explicitly re-exported at the root.
- Existing flat paths remain byte-for-byte name compatible.
- At crate-root child depth, `pub(super)` and `pub(crate)` are both crate-wide. Prefer `pub(super)` as the intent marker for sibling-only use; use `pub(crate)` only when matching an existing declaration or a documented crate-root use makes that spelling clearer.
- Do not add public API for tests or extraction convenience.
- Public signatures, derives, serde/error strings, cfg gates, target gates, and docs remain unchanged.
- `sliding_sync_discovery` re-exports remain explicit and compatible.

## Test redistribution

Move each inline test beside its feature owner. The large mixed `#[cfg(test)] mod tests` disappears only after every test exists exactly once. Raw numeric test ranges must never be swept wholesale: before parent deletion, every worker records the exact moved test-name list and body hash for its destination; the integration owner compares the union to the 143-test baseline and rejects omissions or duplicates.

### Source-characterization migration

Twenty in-lib tests read `include_str!("lib.rs")`. They are assigned as follows:

- client session: `matrix_client_store_config_uses_the_required_key_for_sqlite_builder`, `desktop_client_builder_defaults_enable_thread_subscriptions_and_share_history`, `client_builder_defaults_download_backup_keys_after_decryption_failures`;
- sync: `sliding_sync_invite_probe_contract_is_typed_bounded_and_discards_cursor`;
- E2EE: `recovery_key_path_uses_sdk_signature_publication_only`;
- room projection: `joined_room_list_prefers_async_direct_dm_detection`, `joined_room_list_snapshot_avoids_full_member_scans`, `joined_room_list_dm_resolution_uses_account_data_cached_and_heroes_candidates`, `space_member_ids_are_no_sync_and_space_only`, `joined_only_helpers_do_not_use_active_membership`, `space_lookup_failures_are_not_coerced_to_empty_observations`, `failed_space_member_counts_are_reported_as_unavailable`, `matrix_room_member_summaries_still_scans_full_members`, `live_direct_account_data_loader_is_local_only`, `direct_account_data_dm_detection_fetches_server_when_store_misses`;
- room operations: `mark_room_as_read_sends_read_marker_with_private_receipt`, `cancel_space_invite_validates_invite_membership_before_kicking`, `room_tag_operations_use_sdk_tag_methods`, `pin_operations_use_sdk_pinned_event_methods`, `room_management_wrappers_use_settings_privacy_and_moderation_apis`.

Each reads its single owning file. Tests that previously used the next unrelated top-level item as a textual end marker use `test_source::item_body`, a test-only brace-aware helper that extracts one uniquely named item from the owner source. Assertions and searched production text stay unchanged; only source path and boundary selection change. The two vendor-source `include_str!` calls in `recovery_sdk_records_standard_signature_round_trip_diagnostics` remain unchanged.

Two external whole-crate tests remain whole-crate:

- `tests/send_backup_policy.rs::all_session_constructors_leave_the_per_send_backup_fence_disabled` sums matches over every production source returned by `tests/support/mod.rs` and still requires exactly four false/zero true occurrences.
- `tests/timeline_gap_adapter.rs::committed_room_checkpoint_has_no_legacy_or_room_absent_api` checks every production source for every forbidden token.

`tests/support/mod.rs` contains an explicit, fixed list of `include_str!` inputs for the library production-source universe: `src/lib.rs`, existing `src/sliding_sync_discovery.rs`, and every new non-test library module. It intentionally excludes `src/bin/**` and cfg(test)-only `test_source.rs`. It returns a slice; it does not concatenate or impose a false cross-module order. The existing `tests/send_backup_policy.rs` target hosts a source-contract test asserting that this manifest names every file in that library production-source universe exactly once. The integration owner is explicitly authorized to create this helper and to change only the source-window plumbing in those two external tests.

No new public test hook or standalone integration-test target is introduced.

## Exactness evidence

A temporary non-repository verifier compares the pinned baseline to the integrated tree using brace-aware extraction:

1. Every named top-level production item exists exactly once.
2. Every test name exists exactly once; test-body hashes match except explicit source-path allowlist changes.
3. The explicit root named-re-export set equals the baseline public-item set bidirectionally: every baseline public item appears exactly once, and no private/`pub(super)` item or extra public name appears; `E2eeRecoveryState` remains the retained direct re-export.
4. Attached cfg/doc/derive/serde/target attributes match.
5. Function/method/test bodies, enum variants, struct fields, strings, diagnostic arrays, and match-arm order hash identically after normalization limited to required module qualification, visibility, and the enumerated source-characterization plumbing.
6. The 20 in-lib source tests remain owner-scoped, while the two external aggregate/negative tests scan the exhaustive production-source manifest; no assertion is narrowed or removed.
7. No duplicate production helper, glob, wrapper, compatibility alias, TODO, dead code, or hidden behavioral branch is introduced.
8. Root contains no production declaration/impl/test body/behavioral constant; its cfg(test) support module declaration is allowlisted.

`cargo-public-api` is not installed in this environment, so API evidence uses the explicit source API manifest plus compiler checks across default, `test-hooks`, `smoke`, and all-feature configurations. The recorded baseline commands were executed at the pinned commit: default lib 143/143, `test-hooks` lib 143/143, and all-target/all-feature check green; these results and the source hash are retained in this document and the PR evidence. If the tool becomes available, add it; do not make it a blocker absent from the baseline environment.

## Lifecycle and security invariants

Preserve exactly:

- AccountActor remains the account/session lifecycle and ordered shutdown owner.
- `MatrixClientSession` remains a cloneable SDK adapter, not a task supervisor.
- Verification observer retains, cancels, and joins the same delivery task and handler registrations.
- Undelivered verification leases remain available to later observers.
- Provisional encryption sync owns the exclusive permit for the same stream lifetime.
- Live-tail cancellation, gap checkpoints, and event-cache subscription ownership remain unchanged.
- Storeless first login and keyed restore ordering remain unchanged.
- `close_session_stores` remains the deletion barrier.
- Room-key observer installation order on login/restore remains unchanged.
- Diagnostic counter names/order/dispatch and privacy-safe output remain unchanged.
- No task, handler, stream, permit, token, key, retry, timeout, cleanup, or cancellation owner is added.

## Verification

### Baseline and focused post-move

Run the same gates before and after:

```bash
cargo test -p koushi-sdk --lib
cargo test -p koushi-sdk --lib --features test-hooks
cargo test -p koushi-sdk
cargo check -p koushi-sdk --all-targets --all-features
cargo check -p koushi-sdk --features smoke --bins
cargo fmt --all -- --check
git diff --check
```

During integration, run owner filters for login, secure backup, verification, sync, timeline, search, room list, room settings, profile, and diagnostics.

### Final repository gates

After formal full-diff review:

```bash
cargo test --workspace --all-targets
cargo test -p koushi-desktop --lib
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop test
CHOKIDAR_USEPOLLING=true npm --prefix apps/desktop run test:ui-headless
npm --prefix apps/desktop run build
npm --prefix apps/desktop run lint:tauri-boundary
npm --prefix apps/desktop run lint:domain-deps
npm --prefix apps/desktop run qa:secret-scan
npm --prefix apps/desktop run qa:release-gates
node scripts/check-sdk-submodule.mjs
node scripts/check-agents-docs.mjs
cargo deny check
cargo fmt --all -- --check
git diff --check
```

Run wire/generated artifact checks even though no wire change is allowed. A local homeserver/GUI lane is required only if compilation/tests or review reveal a runtime-path ambiguity; no behavior change may be waived into this PR.

### Final local evidence

- Bidirectional AST/token inventory: 566/566 production keys, 299/299 public declarations and exact root named exports, 143/143 unit-test names; approved source-test plumbing is the only body allowlist.
- SDK package with `test-hooks`: 220 tests across the lib and integration targets; all-target/all-feature and `smoke` bin checks green.
- Rust workspace: 2,394 passed, 13 ignored, 0 failed across 97 suites; desktop lib 149 passed/1 ignored; headless Core QA 129 passed.
- Frontend: typecheck/lint green; Vitest 1,367 passed; UI-headless timeline store 76 passed and Playwright 248 passed with `CHOKIDAR_USEPOLLING=true`; production build green.
- Boundary/policy: Tauri adapter, domain dependencies, secret scan, release gates, SDK submodule, agents docs, IPC generated-wire contract, `cargo deny`, rustfmt, and diff checks green.
- No runtime-path ambiguity was found by compilation, tests, exactness audit, or the formal full-diff review, so the design's conditional local homeserver/GUI lane was not triggered.

## Stop conditions

Stop implementation and amend/re-review the design if:

- an item has ambiguous ownership;
- a worker needs to edit a second worker’s file or the root;
- behavior/body/cfg/public API differs beyond approved qualification/visibility/path changes;
- a wrapper, compatibility shim, duplicate helper, or broadened public API appears necessary;
- lifecycle, cleanup, secret, task, subscription, or retry ownership would move;
- a test exposes a behavior defect.
