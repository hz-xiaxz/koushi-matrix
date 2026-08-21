# Issue #551 runtime profile/display diagnostics extraction

Status: full diff approved; delivery pending. Scope is one behavior-preserving diagnostic projection seam.

## Baseline

- Base: `daede51011b1fbedf388d6bb47feaf0d790ba87f` after connection transport PR #618.
- `runtime.rs`: 9,002 newline-delimited lines / 367,809 bytes / SHA-256 `e9ee3cd88a0f688ce93cb6e80dd6ccd4207135b2e428cd88d7137abc776262c5`.
- Focused baseline:
  - `profile_resolution_diagnostic`: 1/1;
  - `read_receipt_profile_diagnostic`: 1/1;
  - `native_attention_recomputed_diagnostic`: 1 passed / 1 intentionally ignored child;
  - `room_list_applied_records`: 1 passed / 1 intentionally ignored child.

## Ownership decision

Create private direct child `crates/koushi-core/src/runtime/profile_display_diagnostics.rs`. Move exactly these 12 production identities in original relative order:

1. `ProfileResolutionDiagnosticCounts`
2. `has_profile_label`
3. `profile_display_label`
4. `session_user_id`
5. `relevant_room_profile_label`
6. `space_room_profile_label`
7. `local_homeserver_profile_label`
8. `observe_receipt_profile_resolution`
9. `observe_space_member_profile_resolution`
10. `profile_resolution_diagnostic_event`
11. `record_native_attention_recomputed`
12. `live_receipt_profile_diagnostic_event`

Move the complete two-method impl for `ProfileResolutionDiagnosticCounts` in exact order: `observe`, `event`.

The leaf owns pure profile-label resolution accounting, privacy-safe receipt/profile diagnostic construction and native-attention recomputation recording. All mutable data is stack-local; functions borrow `AppState`/`AppAction`/`AppEffect` and retain no handles.

Keep `reduce_with_unread_diagnostics` in `runtime.rs`. It remains the one reducer orchestration boundary that captures pre-reduce room state, emits receipt/profile diagnostics, calls authoritative `reduce`, records unread transitions and then observes returned native-attention effects. AppActor state, reducer mutation/effect ordering, profile/room caches, channels, tasks, timers and shutdown stay parent-owned.

## Privacy contract

Preserve every diagnostic source/stage/field/token/order exactly:

- `core.profile_resolution / resolution`: trigger/cache-status tokens and count fields only;
- `core.read_receipt_profile / resolution`: update/lookup/unresolved tokens plus counts/booleans only;
- `native.attention / recomputed`: observation/badge/candidate/suppression tokens plus counts/booleans only.

No room/event/user IDs, display labels, aliases, avatar MXCs, message bodies, paths or raw SDK errors may be added. Preserve the existing duplicated session-user match; consolidation is out of scope for a move-only PR.

## Imports, visibility and façade

Leaf production has exactly two imports:

1. `koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel, record}`;
2. `koushi_state::{AppAction, AppEffect, AppState, ProfileResolutionInput, ProfileResolutionSource, SessionState, SpaceMemberEntry, SpaceMemberMembership, UserProfile, resolve_people_label}`.

Parent declares private `mod profile_display_diagnostics;` and privately imports exactly three `pub(super)` functions: `live_receipt_profile_diagnostic_event`, `profile_resolution_diagnostic_event`, `record_native_attention_recomputed`. All other moved items and both methods remain private. No public/crate-root re-export, feature namespace, wrapper, alias, trait or compatibility shim is added.

Parent removes exactly six production bindings made orphaned by the move: `ProfileResolutionInput`, `ProfileResolutionSource`, `SpaceMemberEntry`, `SpaceMemberMembership`, `UserProfile`, and `resolve_people_label`. Parent retains diagnostic constructors/recording and all other state types used by its actor/reducer owners.

## Test ownership

Move exactly these two pure owner tests, bodies and attrs unchanged:

1. `read_receipt_profile_diagnostic_reports_child_room_profile_cache_miss`
2. `profile_resolution_diagnostic_counts_actual_resolution_sources`

Leaf tests use `use super::*`, import the existing `super::super::tests::unread_diagnostic_room`, and one explicit state import statement for `LiveEventReceipts`, `LiveReadReceipt`, and `SessionInfo`. The shared helper remains one parent-owned copy with its existing `pub(super)` visibility.

Parent removes only `LiveEventReceipts` and `LiveReadReceipt` from its test import list; `SessionInfo` and `UserProfile` remain required by retained tests.

Retain these wrapper/end-to-end tests in the parent:

- `native_attention_recomputed_diagnostic_records_private_safe_fields` and its ignored child;
- both `room_list_applied_records_*` tests.

Their subprocess `--exact runtime::tests::...` contracts and real reducer/effect ordering remain unchanged.

## Invariants

- Production identities 12/12, methods 2/2 and tests 2/2 move exactly with attrs/comments/field/token/order; parent contains none of their definitions.
- Parent calls exactly three moved functions from `reduce_with_unread_diagnostics`; no wrapper callback or registry is introduced.
- Diagnostic fields remain private-data-free and ordered exactly.
- `reduce_with_unread_diagnostics`, authoritative `reduce`, unread tracing and native-attention effect iteration stay byte-exact in the parent.
- No AppAction/AppEffect/AppState/DTO/serde/wire, reducer, cache, task/channel/timer/shutdown, dependency or public API change.
- No source concatenation, glob in production, visibility widening, duplicate helper, TODO or dead code.

## Deterministic exactness

A temporary `syn` verifier compares immutable base with parent + leaf:

- production 12/12, parent 0;
- methods 2/2 keyed by `(ProfileResolutionDiagnosticCounts, method, name)`;
- moved tests 2/2, parent 0; retained wrapper tests 4/4;
- all 1,029 lib test identities equal after normalizing only two owner paths;
- parent call edges 3/3, `pub(super)` sites 3, leaf imports 2, parent production orphan bindings 6, parent test orphan bindings 2;
- retained wrapper body/effect ordering and shared helper exactly unchanged;
- public API delta 0, resource/lifecycle moves 0, duplicate/missing/excess items 0.

## Verification

Run the same four focused filters before and after, then:

- full core library tests;
- runtime Activity/settings/notification/timeline integration suites;
- `cargo check -p koushi-core --all-targets --all-features`;
- exactness/source/privacy checks, rustfmt and diff checks.

After full-diff approval, integrate latest `origin/main`, obtain delta approval if it moved, and run the complete repository matrix before PR delivery.

## Review gate

- Read-only reconnaissance completed against state/display/unread/diagnostic boundaries.
- `reviewer-flash` independently traced all 12 identities, two methods, three parent calls, imports/visibility/tests/privacy and reducer/resource boundaries and recorded `Correct-to-implement`.
- Implementation re-confirmed the immutable SHA-256, then integrated by `luna-implementer` and parent-audited.
- Exactness: production 12/12, methods 2/2, moved tests 2/2, retained wrapper tests 4/4, all 1,029 test identities, parent calls/`pub(super)` 3, leaf imports 2, production orphans 6 and test orphans 2; public/resource deltas 0.
- `runtime.rs` 9,002 → 8,280 newline-delimited lines; `runtime/profile_display_diagnostics.rs` is 737.
- Focused post-move 1 + 1 + (1/1 ignored) + (1/1 ignored); core lib 1,021/8 ignored, Activity 9, settings 5, notification settings 4 and timeline 21; all-targets/all-features check, rustfmt, exactness and diff checks green.
- Initial full `runtime_timeline` run observed `corrupt_load_attempts_once_per_session` expected 2/got 3; unchanged persistence code, exact-test rerun and complete 21-test rerun were green, recording it as a recurrent timing/isolation failure rather than waiving it.
- Full diff: `reviewer-flash` independently byte-compared all 12 identities, two methods, two moved tests and the retained reducer wrapper, verified privacy/lifecycle boundaries and recorded `Correct-to-merge`.
- Final local evidence: focused diagnostics filters green, core lib 1,021/8 ignored, Vitest 1,370, Playwright 248 with polling, workspace all-targets, desktop 149/1 ignored and Headless Core QA 130; all-targets/all-features, typecheck/lint/build/wasm and all boundary/security/release/wire/SDK/docs/audit/format/exactness/diff gates green.
- Delivery: latest-main integration/delta review if required, PR CI and merge pending.
