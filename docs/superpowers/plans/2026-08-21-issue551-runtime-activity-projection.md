# Issue #551 runtime Activity projection extraction

Status: design review pending. Scope is one behavior-preserving ownership seam.

## Baseline

- Runtime base: `cf9617ec139ea2bf7f795989c7882c906e18f85b` / SHA-256 `2bcb9dcddf31167ddeb778da111c3fe6dd2ccefb19d6a3415c972e54beab3dfb`.
- Focused baseline:
  - `CARGO_PROFILE_DEV_DEBUG=1 cargo test -p koushi-core --lib activity`: 26/26.
  - `CARGO_PROFILE_DEV_DEBUG=1 cargo test -p koushi-core --test runtime_activity`: 9/9.

## Ownership decision

Create private direct child `crates/koushi-core/src/runtime/activity.rs`. Move exactly these 16 production identities in original relative order:

1. `MAX_ACTIVITY_RESOLUTION_ROOMS`
2. `ACTIVITY_RECENT_MAX_ROWS`
3. `activity_tab_token`
4. `record_activity_transition`
5. `ActivityProjection`
6. `ActivityMarkReadResult`
7. `activity_latest_display_event_id`
8. `impl ActivityProjection` with its seven methods in exact order:
   `ingest`, `mark_read`, `fully_read_marker_updates`, `event_at_or_after`, `update_action_for_open_state`, `room_ids_without_remaining_unread`, `snapshot`
9. `room_has_activity_unread`
10. `room_activity_unread_count_for_mode`
11. `activity_recent_row_visible`
12. `activity_row_context_label`
13. `sort_activity_rows`
14. `guard_activity_resolution_completion`
15. `normalize_activity_resolution_action`
16. `cap_activity_resolution_requests`

The leaf owns the bounded account-wide Activity row cache, latest-event reconciliation, unread/context projection, mark-read selection, resolution generation guards and the 16-room request cap. `AppActor` retains the Activity command arms, internal request IDs, AccountActor resolver start/cancel routing, state reduction/effect handling, event emission and central actor loop.

## Unit-test ownership

Move exactly these 11 test functions, bodies and attrs to `runtime/activity.rs` under `#[cfg(test)] mod tests`:

- three `activity_resolution_*` tests;
- eight pure `activity_projection_*` tests.

Keep `activity_mark_read_routes_persistent_room_mark_read_commands` in the parent because it characterizes the central exhaustive command arm. Keep `timestamp_jump_uses_local_activity_projection_before_homeserver_fallback` in the parent because it characterizes navigation command ordering.

The two projection tests using `unread_diagnostic_room` import the existing shared parent test helper. Change that helper visibility only from private to `pub(super)`; do not copy or move it. No other test/helper visibility changes are allowed.

## Visibility and façade

- Parent declares `mod activity;` privately.
- Parent explicitly `pub use activity::ACTIVITY_RECENT_MAX_ROWS;` to preserve `koushi_core::runtime::ACTIVITY_RECENT_MAX_ROWS`.
- Parent privately imports the six directly used symbols: `ActivityProjection`, `activity_tab_token`, `record_activity_transition`, `guard_activity_resolution_completion`, `normalize_activity_resolution_action`, `cap_activity_resolution_requests`; `ActivityMarkReadResult` is inferred and not imported.
- `ActivityProjection`, all seven parent-called methods, `ActivityMarkReadResult` and its two parent-read fields, and the five parent-called functions become `pub(super)` only.
- `MAX_ACTIVITY_RESOLUTION_ROOMS` and all six leaf-only projection helpers remain private.
- No crate-root export, public feature namespace, glob, compatibility alias or wrapper is added.

## Invariants

- All 16 production bodies/tokens, seven method bodies/order, constants 16/200, sort order, relation/latest-event rules, profile/avatar fallback, notification-mode unread policy, cleared-event behavior, generation cap rotation and diagnostic strings remain exact.
- `AppActor.activity_projection` remains the only mutable Activity cache.
- Activity command/effect/event routing, resolver generation, request correlation and reducer ordering remain parent-owned and exact.
- The central `CoreCommand` and `AppEffect` matches remain untouched.
- No command/event/state/serde/wire, privacy/logging, task/channel/timer/cache bound or shutdown behavior changes.
- No duplicate test helper, source concatenation, TODO/dead code or formatting churn.

## Deterministic exactness

A temporary `syn` verifier compares the immutable runtime blob to parent + leaf:

- production 16/16, parent 0;
- `ActivityProjection` methods 7/7 keyed by `(ActivityProjection, method, name)`;
- tests 11/11 in leaf and parent 0 for those names;
- retained parent source/navigation tests 2/2;
- shared helper exactly once with the single approved `pub(super)` delta;
- public path 1/1, parent imports 6, private leaf-only items 7 (one cap constant plus six projection helpers) and explicit approved visibility edges only;
- no duplicate/missing/excess item, glob, wrapper, alias or TODO.

Source-characterization tests continue to read `runtime.rs` individually; moved owner tests do not concatenate source strings.

## Verification

Before and after, run the same focused 26/26 + 9/9 commands above, then:

- `cargo test -p koushi-core --lib`
- runtime Activity, navigation, intent-lifecycle and room-selection integration suites;
- `cargo check -p koushi-core --all-targets --all-features`;
- exactness verifier, rustfmt and diff checks.

After `Correct-to-merge`, run the complete repository matrix recorded in the roadmap before PR delivery.

## Review gate

- Design pending `reviewer-flash` read-only verdict.
- Implementation prohibited until `Correct-to-implement`.
- Full diff and delivery pending.
