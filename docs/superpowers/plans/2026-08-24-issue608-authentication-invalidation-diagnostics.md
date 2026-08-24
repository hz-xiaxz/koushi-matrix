# Issue #608 Authentication Invalidation Diagnostics and UI

## Problem and authority

The Matrix SDK emits `SessionChange::UnknownToken`, but AccountActor currently
keeps only `soft_logout`, dispatches the same `SessionLocked` action used for
E2EE trust loss, and the verification gate renders trust copy. Separately,
`recheck_current_device_trust()` collapses every keys-query error to `Sdk`.
This loses the authentication/trust boundary in state, UI, and diagnostics.

Rust owns the distinction. React only renders a closed Rust DTO; diagnostics
carry only enumerated tokens, booleans, and trust generation.

## Contract

Add a closed Rust-owned lock reason:

```text
SessionLockReason
  DeviceTrust
  UnknownToken { soft_logout: bool }
```

Store it as `AppState.session_lock_reason: Option<SessionLockReason>`, mirrored
verbatim in desktop DTO/TS/fake snapshots. Explicit lock actions record a
reason while entering `SessionState::Locked`: ordinary `SessionLocked` records
`DeviceTrust`; new `SessionAuthenticationInvalidated { soft_logout }` records
`UnknownToken` and performs the same atomic Ready -> Locked cleanup/effects. A
`Locked` state with `None` remains valid for capability-blocked/legacy paths;
there is no `Some(reason) iff Locked` invariant. Unlock, restore, logout,
account switch, reset, and both Locked -> CapabilityBlocked settlement paths
clear it. Add an explicit CapabilityBlocked lifecycle test. Non-Ready
invalidations are inert.
No token, response body, IDs, or server text enters state.

Do not change `SessionState::Locked(SessionInfo)`: the orthogonal field keeps
its stable shape and avoids rewriting every session consumer.

## SDK failure classification

Replace the singleton `CurrentDeviceTrustRecheckError::Sdk` with closed variants
`Authentication`, `Network`, `Server`, and `Sdk`.

Classify `request_user_identity()` errors from structured SDK facts only:

- client API `M_UNKNOWN_TOKEN`, `M_MISSING_TOKEN`, or authenticated 401/403 ->
  `Authentication`;
- `matrix_sdk::Error::Timeout` and `HttpError::Reqwest` -> `Network`;
- remaining HTTP 5xx and structured server/API failures -> `Server`;
- local/unclassified failures -> `Sdk`.

Do not parse Display strings. A 403 authentication request is authentication,
not E2EE trust failure. Record `sdk.current_device_trust_recheck`
`stage=finished outcome=failed failure_kind=<closed token>` at the request seam;
record success as `outcome=success`. The event contains no identity.

### Exact-review classification nuances

- `HttpError::Api` failures map to coarse `Server` unless the structured client
  API kind is `UnknownToken`/`MissingToken` or its structured status is 401/403;
  those map to the diagnostic-only `Authentication` bucket. No display text is
  parsed.
- `Error::AuthenticationRequired` is a diagnostic classification only. It
  never independently emits an authentication-invalidation action or locks a
  promoted session.
- Only the SDK `SessionChange::UnknownToken` signal creates the authentication
  invalidation/lock. Trust-recheck failures, including authentication,
  network, server, and SDK classifications, never do so independently.

## Core correlation and invalidation

`run_session_change_observation` maps SDK `UnknownToken` to an internal closed
`SessionInvalidationReason::UnknownToken`, preserving `soft_logout`, and emits:

```text
core.account stage=session_change_received source=matrix_sdk \
  reason=unknown_token soft_logout=<bool>
```

AccountActor admits the message only while it still owns a session, dispatches
`SessionAuthenticationInvalidated`, stops sync, and emits:

```text
core.account stage=session_invalidated reason=unknown_token \
  soft_logout=<bool> action=lock
```

Trust-recheck completion keeps existing generation fencing and admission
behavior, and adds a settlement event with `generation` and classified
`failure_kind`. The existing generation is the available correlation key; do
not invent a second request counter. A Network/Server/Sdk failure never emits
an authentication-invalidation action and never locks an already-promoted
session. Only the independent SDK `SessionChange::UnknownToken` signal does.

The observer event may precede the admitted event. Move `session_invalidated`
diagnostics after the actor's `self.session` admission check: stale/duplicate
messages after session teardown produce neither a transition nor a false second
admission event, covered by a focused runtime test.

## UI

When the session is Locked with `UnknownToken`, render authentication-specific
heading/copy:

- `Session expired`
- `This session has expired or was revoked. Sign in again to continue.`

Keep only the existing Sign out action for both `soft_logout=true` and `false`;
do not expose password reauth on this locked surface. Render no
verification/recovery/cleanup/bootstrap controls, no secure-backup gate, and no
“Verify this session”/“must be verified again” copy. For `DeviceTrust`, retain
all current verification-gate behavior and copy. Add English/Japanese catalog
entries and pseudo/RTL catalog coverage through existing gates. React does not
infer UnknownToken from timing or diagnostics.

## Verify-first RED / unchanged GREEN

Add closed contract scaffolding and tests before behavior wiring, then capture
behavioral RED (not compile-only):

1. SDK fact classifier: UnknownToken/401 -> Authentication, timeout/transport ->
   Network, 5xx -> Server, local -> Sdk. Initial classifier returns Sdk.
2. State reducer: Ready authentication invalidation atomically locks with
   UnknownToken(true/false), clears session views, resets session status, emits
   the same stop/session/view effects; ordinary trust lock records DeviceTrust;
   unlock/logout/switch and Locked -> CapabilityBlocked clear reason; stale
   non-Ready action is whole-state inert. Initial new action is deliberately no-op.
3. Core observer/actor: both soft-logout values preserve reason and diagnostics;
   classified non-auth trust failure settles without `SessionInvalidated` and
   without locking promoted state. Initial observer route uses old generic lock.
4. UI component: UnknownToken copy/Sign-out-only/control absence for both soft
   values, secure-backup exclusion, and DeviceTrust regression. Initial component
   ignores the new DTO.

Run each unchanged test GREEN after wiring. Diagnostics tests assert exact
component/stage/source/reason/outcome/failure_kind/generation tokens and assert
absence of raw errors/IDs/tokens.

## Files and gates

Expected narrow seams:

- `koushi-sdk/src/e2ee.rs`, exports/tests;
- `koushi-core/src/account/{actor,session_lifecycle,trust_gate}.rs`,
  `runtime/composer.rs`, `runtime/reducer_support.rs`, focused actor/runtime tests;
- `koushi-state` action/state/session/sliding-sync reducers and session tests;
- full mirror chain: `koushi-core/src/state_delta.rs` -> Tauri DTO -> TS -> fake;
  full snapshot uses required `session_lock_reason: SessionLockReason | null`,
  while changed slices use `Option<Option<...>>` / optional TS key so `Some(None)`
  serializes an explicit null clear; schema version 4 remains valid for this
  additive compatible field and two-sided contract/golden tests stay locked;
- `SessionVerificationGate.tsx` tests, TS types, fake defaults, i18n;
- state-machine/ownership docs and this evidence record.

`composer_draft_transition_policy` classifies both lock actions as
`PreservePrevious`; its unit test and one runtime authentication-invalidation
StopSync/effects test keep the trust/auth paths behaviorally identical.

Classifier tests use direct `Error::Timeout` for deterministic Network, mock
keys-query responses for UnknownToken/401 and 5xx, and a local structured error
for Sdk; never rely on an unreliably constructible Reqwest error or live network.

Gates: focused SDK/Core/state/component RED/GREEN; full state session tests,
Core account/trust tests, SDK e2ee tests, desktop typecheck/lint/Vitest, desktop
Cargo check/tests, workspace/all-targets, formatting/docs/diff, CI7/7. No live
server is needed because the SDK error and session-change seams are deterministic.

## Acceptance mapping

| Issue criterion | Proof |
| --- | --- |
| UnknownToken lock reason | observer + reducer + exact diagnostics true/false |
| auth/network/server/sdk classification | structured fact and request tests |
| network is not invalidation | promoted actor/state remains authenticated |
| auth then E2EE are separate | UnknownToken lock reason clears on new session; later trust lock is DeviceTrust |
| gate copy matches action | component tests for both reasons and Sign out |
| required matrix | soft true/false + non-auth failure tests |

Implementation starts only after `reviewer-flash-opencode-go` records
`Correct-to-merge`; exact final diff receives the same post-review.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. The architecture
  was sound; required explicit Locked -> CapabilityBlocked clearing, composer
  draft/runtime action parity, and the complete nested-Option state-delta/DTO
  mirror chain. These are incorporated above, together with deterministic
  classifier seams, post-admission diagnostics, and control-absence coverage.
- Round 2: `Correct-to-merge`. Every prior seam and nested clear encoding was
  verified against current code. The legacy, product-unreachable
  `AppAction::SyncFailed { reason: "sync_failed_auth" }` reducer remains outside
  this explicit-reason invariant; do not infer UnknownToken from that string.

## Implementation evidence

- Verify-first RED was captured after adding the closed contract scaffolding but
  before behavior wiring: SDK structured-fact classification, state auth-lock
  lifecycle, Core observer/admission diagnostics, and UI auth copy/control tests
  all failed behaviorally rather than at compile time.
- GREEN: state session lifecycle 82/82; SDK trust-recheck classifier/request 4/4;
  Core UnknownToken observer/admission/post-teardown diagnostics 4/4; Core
  non-auth Network settlement 1/1; nested state-delta clear 1/1; UI gate +
  appStore 53/53; Tauri frontend golden 1/1; typecheck GREEN.
- Both `soft_logout=true` and `false` produce exact privacy-safe
  `session_change_received source=matrix_sdk reason=unknown_token` and admitted
  `session_invalidated reason=unknown_token action=lock` events. A post-teardown
  message emits neither action nor admitted event. Network trust failure records
  classified `failure_kind=network` with generation and does not dispatch an
  authentication lock. UnknownToken now also stops provisional trust observers
  and increments the trust generation; a late in-flight `Verified` completion
  is fenced and cannot unlock the invalid authentication session (1/1 GREEN).
- The full mirror chain carries `session_lock_reason`; explicit null state-delta
  clears are tested. UnknownToken renders **Session expired** and sign-out-only
  controls; DeviceTrust retains the verification copy.
- Full frontend gates are GREEN: Vitest 1457/1457, typecheck/lint/build, strict
  Tauri golden, formatting/docs/diff/submodule checks. The rendered
  browser-headless UnknownToken sign-out-only scenario is 1/1 GREEN and the
  complete browser-headless matrix is 261/261. State all-targets, Core 1029/1029
  (8 ignored), and SDK e2ee 74/74 are GREEN. The first browser run's sole font
  failure was a worktree symlink outside Vite's allow-list; worktree-local
  hardlinked dependencies made the unchanged focused and full reruns GREEN.
- Rebased conflict-free in production code onto merged #659/main
  `3a76a9e8bec62f122a76404afdaa61eac399d3b7`; the sole conflict was the plans
  index and retained both #659/#608 rows. Post-rebase focused GREEN: state
  session 82/82, Core UnknownToken 5/5, SDK UnknownToken 4/4, and frontend
  session/gate/isolation 41/41. Full final matrices remain required after any
  later merged-main rebase before exact review/PR.

## Follow-up validation: Tauri IPC default snapshot

- RED test: `npm --prefix apps/desktop run test -- src/test/tauriIpcMock.test.ts`
  (2 passed, 1 failed).
- Exact failure: `AssertionError: expected { …(31) } to have property
  "secure_backup_gate"` at `src/test/tauriIpcMock.test.ts:95:22`.
- GREEN: the unchanged focused mock test is 3/3; focused SessionVerificationGate /
  App tests are 124/124; desktop typecheck exits 0; `git diff --check` exits 0.

## Substitute exact-review Minor resolution

- The user approved `reviewer-flash` as the mandatory substitute after the
  original reviewer exhausted its monthly quota. Its decomposed exact review
  returned `Correct-to-merge` with non-blocking evidence gaps/wording findings;
  all actionable findings are resolved here before final re-review.
- Browser Fake login failure previously left a stale UnknownToken reason after
  returning `SignedOut`. The focused test was RED (exit 1: expected null, got
  `UnknownToken`) against the pre-fix fake and is GREEN unchanged after the
  one-line lifecycle clear. This is fake parity only; Rust already cleared it.
- New deterministic evidence pins the existing production guards: an
  unpromoted-but-owned session ignores UnknownToken without action/admitted
  diagnostic (1/1), and runtime integration proves `SessionLocked` plus
  authentication invalidation for both soft-logout values each execute the same
  StopSync route (1/1 table test). Focused Browser Fake is 144/144; typecheck,
  docs, and diff checks are GREEN.
- State-machine wording now permits representational `Locked + None` for
  legacy/manual restored state while correctly identifying `CapabilityBlocked`
  as a distinct variant. Structured 401/403 or UnknownToken/MissingToken facts
  may classify a trust recheck diagnostically as Authentication, but only SDK
  `SessionChange::UnknownToken` dispatches authentication invalidation.
- Final Minor coverage pins both capability-blocked clear sites by seeding a
  reason before initial blocking and before in-flight revalidation settlement;
  both end with `session_lock_reason == None`. A deterministic mock keys-query
  403/M_FORBIDDEN response pins the structured-status Authentication diagnostic
  branch without inferring invalidation. These are test/doc-only additions; no
  production behavior changed.
- After rebasing onto merged #582/main, the mandatory exact reviewer identified
  one preserved test-fixture drift: the default Tauri IPC Space-members slice
  omitted #582's required `power_levels_revision` and `can_edit_roles` fields.
  The fixture and its complete-contract assertion now include the Rust-shaped
  null/false defaults; this is test-only parity and changes no product behavior.
