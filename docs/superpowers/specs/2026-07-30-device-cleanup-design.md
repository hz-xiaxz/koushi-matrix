# Failed Verification Device Cleanup Design

Date: 2026-07-30
Issue: #370
Status: Approved through the user's standing approval for the #9-to-#286 issue run

## Scope

PR #378 already disabled device-to-device/SAS verification in the production
verification gate. This design covers the remaining #370 cleanup contract:

- an explicit `Cancel sign-in and remove this device…` action;
- a consequence-list confirmation before any destructive work;
- remote device/session removal before local persistence deletion;
- password UIAA for legacy Matrix sessions and OAuth token revocation for
  OAuth/MAS sessions;
- idempotent already-absent handling;
- a retryable remote failure that retains the credentials needed to retry;
- a separately confirmed local-only escape hatch after remote failure;
- private-safe lifecycle diagnostics available before Ready.

The live session-status surface, homeserver account-management destination, and
OAuth device naming remain #369. In particular, `oauth_device_name` diagnostics
belong to #369, where the device name is selected. This PR must not create a
second owner for that behavior.

## Existing Defect

`SessionVerificationGate` currently reveals `Reset local data for a new
device` after a verification failure and calls `reset_local_data` directly.
That path has no server-side deletion or token-revocation attempt. It can erase
the only local credentials capable of retrying remote cleanup while leaving the
unverified device registered on the homeserver.

The existing building blocks are individually useful:

- `koushi-sdk::delete_devices` supports legacy Matrix device deletion and
  returns an opaque UIAA continuation;
- `koushi-sdk::logout` already distinguishes OAuth sessions and revokes OAuth
  tokens instead of assuming password UIAA;
- `AccountActor::handle_reset_local_data` owns ordered runtime shutdown and
  local account-persistence clearing.

They are not currently joined by a Rust-owned cleanup state machine.

## Upstream Reference

Element Web uses an explicit `Remove this device` confirmation. Its device
manager first calls the Matrix multi-device deletion endpoint and enters
interactive authentication only after a server UIAA challenge. Its native OIDC
logout tests require both access-token and refresh-token revocation. Element X
Android treats sign-out as an authentication-service concern and clears the
local session after the Matrix client sign-out attempt.

Koushi follows those boundaries but intentionally differs on failure: because
this action repairs a provisional, unverified sign-in, a remote failure does
not automatically clear local credentials. The user gets retry and an explicit
`Erase local data anyway` escape hatch.

## Considered Designs

### A. Dedicated Rust-owned cleanup state machine (selected)

Add an AppState cleanup slice alongside the provisional verification gate, typed cleanup
commands, AccountActor-owned UIAA continuation, SDK authentication-mode
classification, and reducer-correlated settlement.

This satisfies the repository's state ownership, retry, privacy, and
remote-first rules. It also makes browser and Tauri code transport-only.

### B. Reuse ordinary logout, then call local reset

The ordinary logout path is best-effort by design and proceeds with teardown
after server failure. It cannot expose legacy UIAA, already-absent settlement,
or retryable failure without changing normal logout semantics. Rejected.

### C. Chain delete/logout and reset promises in React

This would put Matrix command ordering, pending state, failure classification,
and retry semantics in the least-trusted layer. It also risks losing state on a
rerender or WebView restart. Rejected.

## State Contract

`AppState` gains a `device_cleanup` field so failures that happen before
`VerificationGateState` discovery are also representable:

```text
Idle
Offered { reason }
ResolvingRemote { request_id }
RemovingRemote { request_id, auth_mode }
AwaitingUia { request_id, flow_id }
RemoteFailed { request_id, auth_mode, failure_kind }
ResettingLocal { request_id, remote_outcome }
LocalResetFailed { request_id, remote_outcome, failure_kind }
ErasingLocalAnyway { request_id }
```

The field is serialized through the normal AppState/Tauri/TypeScript snapshot
contract. It contains no Device ID, user ID, homeserver, token, UIAA session, or
raw SDK error.

`Offered` is entered only after a classified verification/recovery failure or
the established no-proof-method gate. Merely being unverified never starts
cleanup. Opening or closing the confirmation dialog is ephemeral presentation
state and remains React-owned.

The cleanup slice is reset when the provisional session is promoted, rejected,
logged out, switched, or replaced by another login attempt.

## Commands and Guards

New account commands:

- `StartDeviceCleanup { request_id }`
- `SubmitDeviceCleanupUia { request_id, flow_id, password }`
- `EraseLocalDataAnyway { request_id }`

`StartDeviceCleanup` is admitted from `Offered`, `RemoteFailed`, or
`LocalResetFailed`. From `LocalResetFailed` it retries only the local stage;
remote deletion/revocation is not repeated after it has already settled.
Repeated clicks while remote removal or local reset is pending are rejected.

`SubmitDeviceCleanupUia` is admitted only for the matching `AwaitingUia`
`flow_id`. The password is an `AuthSecret`; it exists only in the DOM, Tauri
command, CoreCommand, and actor call. It is never reducer state or an event.

`EraseLocalDataAnyway` is admitted only from `RemoteFailed`. The GUI gives it a
second confirmation that explicitly says the remote device may remain.

Every actor-side terminal path sends a matching reducer action. A generic
`OperationFailed` event is supplemental and cannot be the only settlement.
Stale request IDs and stale UIAA flow IDs are ignored by the reducer and
rejected by the actor.

## Authentication and Remote Cleanup

`koushi-sdk` exposes a coarse `SessionAuthMode` (`Legacy` or `OAuth`) derived
from the active SDK client's session, plus a cleanup primitive returning:

```text
Success
AlreadyAbsent
UiaaRequired { opaque session }
Failed { private-safe kind }
```

### Legacy

The actor resolves the authoritative current Device ID from its active
`MatrixClientSession`, not from WebView input, and calls the existing device
deletion endpoint.

The first attempt carries no auth so the server can return its UIAA challenge.
Only a legacy challenge may project `AwaitingUia`. The UI submits the password
through the dedicated continuation command. OAuth sessions never enter this
state.

`M_UNKNOWN_TOKEN` and authoritative not-found responses settle as
`AlreadyAbsent`; they mean there is no usable remote device/session left for
these credentials. Other server, network, timeout, and SDK failures become
coarse retryable failure kinds.

### OAuth/MAS

The actor calls the SDK OAuth logout/revocation path. It does not call Matrix
password UIAA or render a password field. Successful access/refresh token
revocation settles `Success`; an authoritative invalid/absent session settles
`AlreadyAbsent`.

OAuth account-management navigation is not a substitute for programmatic
revocation. If revocation fails, cleanup remains `RemoteFailed` and #369's
account-management destination can be used as an external fallback.

## Remote-First Ordering

The actor performs these stages in order:

1. classify the active session authentication mode;
2. settle remote deletion/revocation as `Success` or `AlreadyAbsent`;
3. project `ResettingLocal`;
4. stop provisional runtime/observers;
5. clear keyed SDK stores, cached account data, saved credentials, and local
   unlock material;
6. project `SignedOut`.

No local deletion function is called before stage 2 succeeds. On a remote
failure the session, key ID, SDK client, credential vault entry, and encrypted
store remain available for retry.

For `EraseLocalDataAnyway`, stages 1 and 2 are deliberately skipped only after
the second confirmation. The resulting signed-out state makes no claim that
the remote device was removed.

Local deletion reports a coarse aggregate result after attempting every
account-scoped credential and directory removal. Failure settles
`LocalResetFailed` and does not project successful completion. The actor
retains the account key needed to retry the local stage; it does not need or
attempt to restore the now-remotely-invalid SDK session.

## Diagnostics

Use source `device_cleanup`, record to stderr and the normal diagnostic
collection, and attach request correlation. Allowed events are:

```text
stage=offered reason=recovery_failed|no_proof_method
stage=confirmed auth_mode=legacy|oauth|unknown
stage=remote_delete_started
stage=remote_delete_settled outcome=success|already_absent|failed failure_kind=...
stage=local_reset_started
stage=local_reset_settled outcome=success|failed
stage=completed outcome=signed_out_new_device_required|local_only_remote_may_remain
```

Fields are limited to request IDs, enum tokens, booleans, and elapsed
milliseconds. Device/user IDs, homeservers, UIAA sessions, tokens, passwords,
recovery material, and raw SDK errors are forbidden.

The already-shipped SAS availability behavior remains the owner of
`verification_ui` diagnostics. `oauth_device_name` is implemented and verified
with #369, not duplicated here.

## Trust Semantics

The current admission path subscribes to the SDK's aggregate current-device
verification state, requests an authoritative own-identity refresh, and
promotes only an authoritative `VerificationState::Verified`. It does not call
an own-device `is_verified()` predicate. #370 adds a regression test that pins
this boundary and documents that the cleanup offer does not itself imply
verified or unverified trust.

The richer live trust facts and their UI belong to #369. #370 must not invent a
React-local trust model while waiting for that issue.

## GUI

After a recovery/no-proof failure, the gate renders:

- the existing classified failure and recovery retry;
- `Cancel sign-in and remove this device…`;
- a confirmation dialog listing remote removal/token invalidation, local
  encrypted store/cache/credential deletion, homeserver-message preservation,
  irreversibility, and new Device ID on next login.

`RemovingRemote` and `ResettingLocal` disable duplicate submission and show
Rust-owned progress. `AwaitingUia` renders an IME-safe password form. A
`RemoteFailed` state renders the classified error, `Retry removal`, and
`Erase local data anyway`; the latter opens its own warning confirmation.

All visible strings use the i18n catalog. React may own dialog visibility and
the transient password DOM value, but not cleanup status or outcome.

## Verification

The same tests that first fail must then prove:

- reducer happy path, failure, stale correlation, duplicate settlement, UIAA,
  OAuth-without-UIAA, already absent, and local-only escape transitions;
- SDK legacy/UIAA and OAuth classification without raw-error exposure;
- AccountActor remote-before-local call order and preservation on failure;
- Tauri/TypeScript snapshot and command wire contracts;
- component production-default SAS policy plus cleanup confirmation copy;
- browser-headless success, remote failure/retry, already absent, and
  local-only escape;
- diagnostic token inventory contains no identifiers or secrets;
- authoritative trust refresh remains aggregate verification-state based.

Focused gates precede the one integrated local homeserver scenario. The final
branch review includes tracked and untracked files, SDK submodule guard, Rust
workspace tests, desktop typecheck/lint/tests, and the relevant Playwright
specs.
