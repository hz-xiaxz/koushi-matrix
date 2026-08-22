# Issue #641 Browser Fake Session View Reset

## Scope

Fix one lifecycle owner before further `browserFakeApi.ts` decomposition: `clearSessionViews` must revoke all browser-fake account/session projections and resource maps when a session ends or is replaced, and an initially locked fake must expose the same cleared view boundary. No declaration moves, API/DTO changes, production Rust changes, or unrelated `appHarnessMain.tsx` work.

Immutable baseline: main `3bb63c3174b950348727ed253c39168ee287b2dd`; `browserFakeApi.ts` 6,306 lines / 209,570 bytes / SHA-256 `6bfb89900180e45581c40e18adac1c91666a90d9639806e1490bd5830a2c8530`; test 2,615 lines / 90,998 bytes / SHA-256 `0ee64ac86164eb92ec41aedd011638131ed83ed73256968276d6f4f8d0937fae`.

## Contract boundary

Rust `clear_session_views` in `crates/koushi-state/src/reducer/mod.rs` is authoritative for session-owned view slices. The browser fake collapses asynchronous terminal transitions into returned snapshots, so it must additionally settle account status/gate fields that Rust resets in surrounding reducer actions or final `AppState::default()` replacement.

Preserve process/device settings and derived display policy during the shared clear operation: `settings`, `locale_profile`, `typography_profile`, and `cjk_text_policy`. Preserve pre-login `auth` in the shared operation because failed login may reuse discovery; the browser fake intentionally retains that discovery projection on its collapsed terminal paths rather than emulating Rust's later full-state replacement. Do not reset instance-monotonic request/lease counters.

## RED proof

Add a table-driven browser-fake test that, for each non-replacement terminal path (`logout`, `changeHomeserver`, failed `submitLogin`, `resetLocalData`):

1. starts from a ready fake;
2. dirties publicly mutable session slices with existing APIs: current-session status, account-management capability, link-preview override/room preference, space members, room notification mode, room interaction, mention candidates, invite workflow, search crawler, live signals, local-encryption health, Threads list, Files view, and `main_timeline_anchor`;
3. observes the terminal snapshot;
4. asserts canonical values for every reset slice below, the exact navigation literal `{ active_space_id: null, active_room_id: null, space_order: [], last_room_by_space_id: {} }` with optional anchors/memory absent, and exactly one `login_failed` error only on failed login.

Before the fix this test must fail on multiple publicly reachable stale values. Also retain/extend focused assertions that `completeOidcLogin` and `switchAccount` replacement paths return canonical ready snapshots. Add a locked-constructor RED proof: `createBrowserFakeApi({ session: "locked" })` must contain locked session identity but no ready-session rooms, spaces, navigation, timeline, views, member/live/account projections, or fixture data.

Fields without a public nonterminal mutator (`native_attention`, `thread_attention`) still receive explicit canonical assertions; do not add test-only production mutation hooks.

## GREEN implementation

Extend the existing `clearSessionViews` owner directly. Keep its resource teardown order first, then reset state. Add or correct exactly these omissions:

- surrounding account/session terminal state: `secure_backup_gate = { kind: "inactive" }`, `current_session_status = { status: "idle" }`, `device_cleanup = { kind: "idle" }`;
- navigation: include `space_order: []` while replacement removes optional anchors/memory;
- `link_preview_settings = { room_overrides: {} }`;
- `room_preferences = { rooms: {} }`;
- `space_members = emptyBrowserFakeSpaceMembersState()`;
- `invite_workflow = defaultInviteWorkflowState()`;
- `room_notification_settings = {}`;
- `room_interactions = {}`;
- `mention_candidates = { targets: [] }`;
- `thread_attention = { kind: "closed" }`;
- `search_crawler = { rooms: {}, last_active: null }`;
- `live_signals = defaultLiveSignalsState()`;
- `local_encryption = { kind: "unknown" }`;
- `native_attention = defaultNativeAttentionState()`;
- `account_management_capabilities = { change_password: { kind: "unknown" } }`;
- `state.ui.threads_list = { kind: "closed" }`;
- `state.ui.files_view = { kind: "closed" }`.

Keep all six callers and their replacement ordering unchanged. `submitLogin` must retain exactly its newly generated `login_failed` error; do not globally clear `ui.errors` in `clearSessionViews`.

Correct `createLockedSnapshot` in the same lifecycle-owner change: base it on `createSignedOutSnapshot(secureBackupGate)`, then install the locked saved-session identity. Do not call class-private teardown from a constructor, duplicate the reset payload, or change `needsRecovery` behavior. Initial locked state has no submission history to preserve.

## Deterministic checks

- RED failure and same exact test GREEN x3.
- Browser fake and client focused suites.
- Source check: six `clearSessionViews` call sites unchanged; locked constructor uses the signed-out basis; no API/class field/timer/map/export delta.
- Full frontend, Rust, Tauri, Headless Core QA, wasm, boundary, security, docs, formatting, dependency and diff gates.
- Post-implementation full-diff review, latest-main comparison, CI 7/7, merge, #641/#551 evidence and cleanup.

## Review gate

Revision 1 reviewer verdict: `Correct-to-implement`. Revision 2 adds the reviewer-identified locked-state parity gap and coverage clarifications. Revision 2 reviewer verdict: `Correct-to-implement`.
