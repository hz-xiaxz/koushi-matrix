# Issue #551 runtime decomposition roadmap

Status: roadmap approved. The split-later prerequisite (SDK/Room/Account/Timeline actors and TimelineView seams) is complete.

## Immutable baseline

- Base: `cf9617ec139ea2bf7f795989c7882c906e18f85b`.
- `crates/koushi-core/src/runtime.rs`: 10,909 newline-delimited lines / 441,487 bytes.
- SHA-256: `2bcb9dcddf31167ddeb778da111c3fe6dd2ccefb19d6a3415c972e54beab3dfb`.
- No production `unsafe`.
- Current public paths from both `koushi_core::runtime::*` and crate-root re-exports are compatibility contracts.

## Ownership and lifecycle boundary

`CoreRuntime` owns the bounded command/action/event channels, watch snapshot, composer lease registry, media preparation service and two abort-on-drop task handles. `AppActor` owns the authoritative `AppState`, central select loop, persistence load markers, composer debounce, scheduled-send timer, activity projection, navigation projection fences, command routing and both command-origin/post-projection effect registries.

The ordered shutdown contract remains:

1. close command admission;
2. let `AppActor::run` terminate;
3. flush pending composer drafts;
4. send AccountActor shutdown and await its ordered child/store teardown;
5. await AppActor;
6. await the media reconciliation task;
7. retain `AbortOnDrop` as the abnormal-drop fallback.

No PR may move timer polling out of the one select loop, detach a task, split AppState ownership, reorder state publication/events, merge the two effect registries, or change composer permit fail-closed behavior.

## Planned independently mergeable seams

1. **Activity projection** — pure account-wide row cache, unread/context projection, resolution normalization/cap and its unit tests. Leave Activity command arms and resolver dispatch in the exhaustive façade.
2. **Connection transport/admission façade** — `CommandSubmitError`, connection/command handles, lag projection and `CoreRuntime::attach`; keep the inbox envelope, `CoreRuntime`, abort-on-drop task guards and shutdown owner in the parent. Preserve all public paths through explicit re-exports and add no visibility edge.
3. **Profile/display diagnostics** — pure profile-resolution and display-label diagnostic projection; no actor state/task movement.
4. **Composer draft lifecycle** — admission identities, forwarded permit lifecycle, encrypted load/persist/reconciliation and debounce helpers atomically; keep timer select arm in the actor loop.
5. **Navigation support** — persistence, replacement cleanup and focused projection-ack helpers; keep exhaustive navigation command registry and latest-desired queue in the actor, and preserve the tested ordering in which local `activity_projection` lookup precedes `AccountMessage::OpenTimelineAtTimestamp` fallback.
6. **Scheduled sends** — local persistence/deadline/due-dispatch helpers atomically; keep schedule/cancel/reschedule command arms central.
7. **Reducer/projection support** — reducer instrumentation and deferred persistence implementation; preserve one actor loop and both exhaustive effect dispatchers.
8. **Final residual audit** — retain `AppActor`, `run`, `handle_command`, `handle_app_effects`, `handle_post_projection_effects`, consumer event projection, verification allowlist and account speculative projection registry if further movement requires wrapper APIs or duplicate ownership.

Each seam gets a separate reviewed plan/diff/PR. The order may change only when the prior seam reveals a concrete import/visibility dependency; no two write-capable workers share a worktree.

## Central exhaustive registries that remain visible

- `AppActor::run`: channel batching, timers, lease changes and shutdown ordering.
- `AppActor::handle_command`: exhaustive `CoreCommand` and ready-session admission.
- `handle_app_effects`: command-origin side effects.
- `handle_post_projection_effects`: actor-origin firewall against duplicate side effects.
- `CoreConnection::project_event_for_consumer`: exhaustive consumer projection.
- `is_verification_gate_command`: closed provisional-session allowlist.
- `account_command_projected_action`: privacy-safe speculative account projection.

Static/exhaustive registries remain centralized unless the final residual review proves one atomic routing owner.

## Exactness and verification strategy

Use the immutable Git blob with a temporary `syn` verifier. Key top-level items by kind/name and associated items by `(normalized self type, item kind, name)`. Compare production bodies/tokens, test names/attrs/bodies, visibility/cfg/signatures, constants/strings/order and public/crate paths bidirectionally. Allow only approved import qualification, explicit `pub(super)` sibling/parent edges, module declarations/re-exports and owner-file source-test path changes.

Reject duplicate logic, wrappers, compatibility shims, glob exports, one-implementation traits, TODO/dead code, unapproved visibility and missing/excess items. Record channels/tasks/timers/maps and teardown owners per PR.

Focused commands are selected per seam. Every final PR also runs workspace all-targets, Core QA binary, Tauri/wire checks, frontend typecheck/lint/Vitest/build/Playwright, wasm, SDK/agents docs, rustfmt, cargo-deny/machete, secret/release gates and diff checks.

## Review gate

- Reconnaissance completed read-only against command/event/state/account/SDK boundaries and runtime integration suites.
- Formal `reviewer-flash` review traced the lifecycle/registry/public/test graph and recorded `Correct-to-implement` for this roadmap and the first Activity seam.
- The review's later-navigation ordering note is recorded above; implementation may proceed only under each seam's own reviewed plan.
