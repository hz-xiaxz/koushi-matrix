# Required Simplified Sliding Sync And Diagnostics Design

Date: 2026-08-03
Status: Approved design for issue #412
Issue: https://github.com/shinaoka/koushi-matrix/issues/412

## Decision

Koushi requires Element X-compatible Simplified Sliding Sync. Production owns
one Matrix Rust SDK `SyncService`, one unfiltered `all_rooms`
`RoomListService`, and the encryption sync connection supervised by that
service. Legacy `/sync`, backend selection, fallback, forced-Legacy QA, and
Conduit-positive support are removed.

The migration lands as three sequential PRs. Issue #412 remains open until all
three are merged and the final requirement-by-requirement audit is complete.

1. Establish Tuwunel and Synapse positive invitation lanes and freeze the
   Element X request contract while Legacy still exists.
2. Add the typed capability gate, make SyncService/RoomListService mandatory,
   and remove all production Legacy runtime and wire states.
3. Add user-copyable diagnostics, remove Conduit and obsolete QA surfaces,
   finish active-canon migration, and run the integrated gates.

Each PR starts from the then-current `origin/main`, is independently reviewed,
and is merged before the next begins. No stacked unmerged implementation
branches are required.

## Evidence And Compatibility Baseline

Element X Android and iOS 26.07.28 pin matrix-rust-sdk commit
`ccd225e58eb900e321411397d1c13c2d9b312bb6`. At that revision,
`RoomListService` builds:

- connection ID `room-list`;
- exactly one list named `all_rooms`;
- no invite-only filter (`is_invite` is unset and omitted on the wire);
- list timeline limit 1;
- the room/member/space state required for room previews and classification;
- account-data, receipt, typing, and applicable thread-subscription
  extensions; and
- a separate encryption sync connection supervised by `SyncService`.

The checked-out Koushi SDK gitlink `341107e26621c614b71f9528eb5ce3fa39b3df41`
has the same request skeleton plus narrow Koushi provenance patches. It can
satisfy the Element X request contract without a wholesale SDK rebase. The
parity guard compares serialized request structure, not source text or SDK
revision labels.

The public service state is insufficient to prove the first committed
successful response: `SyncService::State::Running` describes task lifecycle,
not response commit. Koushi therefore adds one minimal upstream-shaped SDK
observable after the room-list sliding-sync response and event-cache mutation
commit. It exposes only a process-local response sequence and `pos_present`
boolean. It exposes neither the position value nor Matrix identifiers. The
patch is recorded in `docs/upstream/matrix-rust-sdk-feedback.md`; it does not
change request semantics.

## Canon Reconciliation

Current active canon requires behavior-probed invite-only sync and Legacy
fallback. That requirement is superseded by this design. Before final-runtime
code lands, the owning PR amends:

- `REPOSITORY_RULES.md` — Tuwunel/Synapse headless-first server policy and
  single Simplified Sliding Sync ownership;
- `docs/architecture/overview.md` — Async rule 10, SyncActor/RoomActor,
  timeline provenance, gap repair, and QA model;
- `docs/architecture/state-machine.md` — capability/session blocking,
  sync-mode removal, room-list readiness, and invite ownership;
- `docs/policies/engineering-rules.md` — capability discovery, diagnostics,
  local gates, timeline provenance, and removal of the authenticated
  invite-probe rule; and
- relevant `docs/qa/` contracts and launch commands.

The engineering policy currently says root Matrix SDK dependencies use a
remote rev while `REPOSITORY_RULES.md` and `AGENTS.md` require exact submodule
paths. The submodule-path rule is authoritative and already enforced by
`scripts/check-sdk-submodule.mjs`; PR 1 corrects the stale engineering-policy
wording before parity work proceeds.

Historical dated specs remain as records but receive an explicit
`Superseded by issue #412` status note where an active reader could otherwise
mistake their Legacy fallback design for current policy.

## Capability Discovery

### Ownership And Type

`koushi-sdk` wraps the SDK-native `/_matrix/client/versions` request and maps
it to an app-owned result without returning raw HTTP or SDK errors:

```rust
pub enum SlidingSyncDiscoveryResult {
    Supported {
        source: DiscoverySource,
        advertised: bool,
        http_status_class: Option<HttpStatusClass>,
    },
    Unsupported {
        advertised: bool,
        http_status_class: Option<HttpStatusClass>,
    },
    Unreachable {
        failure: DiscoveryTransportFailureKind,
    },
    InvalidResponse {
        failure: DiscoveryResponseFailureKind,
        http_status_class: Option<HttpStatusClass>,
    },
}
```

`Supported` requires
`unstable_features["org.matrix.simplified_msc3575"] == true`. A successful
response with the key absent or false is `Unsupported`. DNS, TLS, proxy,
connection, timeout, and HTTP transport failures are `Unreachable`. A
response that cannot be decoded or used as the typed versions response is
`InvalidResponse`.

Exact status codes, response bodies, homeserver URLs, and raw errors do not
cross the adapter. The only optional HTTP fact is the coarse `2xx`, `4xx`, or
`5xx` class.

### Shared Admission Gate

`AccountActor` owns one request-correlated capability gate used by:

- password login before credential exchange;
- OIDC/MAS authorization start before dynamic registration/browser handoff;
- OIDC/MAS callback completion as a stale-flow revalidation fence; and
- stored-session restoration before normal child actors start.

New authentication requires a current network `Supported` result before the
login can be committed. `Unsupported` produces the localized unsupported-
homeserver result. `Unreachable` and `InvalidResponse` produce distinct
retryable failures and never reuse unsupported copy.

An accepted positive result is persisted beside the secret session metadata
as a boolean positive-support cache and probe time. No negative result is used
as a durable deny-list. This cache exists so a previously supported stored
session can restore its encrypted store and render stale cached rooms while
offline. It never bypasses the initial gate for a new login.

Stored-session behavior is:

```text
network Supported
  -> refresh positive cache -> continue restore
network Unsupported
  -> CapabilityBlocked(unsupported) -> preserve credentials and stores
network Unreachable/Invalid + cached Supported
  -> continue cache-only restore -> schedule network revalidation
network Unreachable/Invalid + no cached Supported
  -> CapabilityBlocked(retryable kind) -> preserve credentials and stores
```

`CapabilityBlocked` is an authenticated-local, non-Ready session state. It
authorizes only retry discovery, sign out, change homeserver/account, and local
data management. It starts no Sync/Room/Timeline/Search actor. A successful
retry resumes the normal trust/admission flow with the same credentials and
stores. Unsupported, unreachable, and invalid responses never delete local or
remote session state.

If revalidation of a cache-admitted session later proves `Unsupported`,
`AccountActor` performs ordered child shutdown and enters
`CapabilityBlocked(unsupported)` without clearing persistence. Ordinary
offline retries remain `Ready + Reconnecting`; they are not capability
failures.

### State-Machine Guards

Capability attempts carry account/session epoch plus request ID. Duplicate or
stale completions are ignored. Logout, account replacement, and change-
homeserver retire the attempt. Only a matching `Supported` completion can
leave `CapabilityBlocked`; a new retry clears the previous presentation
failure only after its request is accepted.

### Provisional Device Verification

The current verification gate uses a filtered classic `/sync` owner before a
device reaches `SessionState::Ready`. That production request also falls under
the #412 removal requirement; removing only the normal Legacy backend would
not satisfy “Koushi never sends production `/v3/sync` requests.”

The provisional gate is migrated to the SDK `EncryptionSyncService`, which is
itself a Simplified Sliding Sync connection carrying the E2EE/to-device data
needed for trust discovery and SAS. `AccountActor` owns this restricted
provisional service. It cannot publish normal room, timeline, search, or
attention projections. It is cancelled and joined before `Ready` is projected.
Only after that barrier may `SyncActor` build and start the account's single
normal `SyncService` and its room-list/encryption pair.

The two owners are sequential, never concurrent:

```text
authenticated provisional session
  -> restricted EncryptionSyncService only
  -> authoritative device Verified
  -> stop and join restricted service
  -> project Ready
  -> build/start exactly one normal SyncService
```

The production `SyncOnce` command and filtered classic-sync route are removed.
Low-level SDK test helpers may remain only when compile-gated to tests/QA and
unreachable from a release runtime. The final source and request-capture gates
prove that no production path can send `/_matrix/client/v3/sync`.

## Sync Ownership And Connectivity

`SyncActor` has no backend enum and no selection function. Its established
session state owns:

- exactly one `Arc<SyncService>`;
- exactly one `Arc<RoomListService>` obtained from that service;
- one SyncService supervisor task, which owns the room-list and encryption
  connections;
- a monotonic Core sync generation; and
- the latest committed SDK response sequence for that generation.

Starting constructs both services before publishing handles. `RoomActor`
receives the `Arc<RoomListService>` as a required constructor argument. A
partially constructed service set is dropped and reported as a typed startup
failure; no actor starts with `Option<RoomListService>`.

The lifecycle projection remains actor-owned and latest-wins:

```text
Stopped -> Starting -> Running
Starting/Running -> Reconnecting on Offline or retryable Error
Reconnecting -> Running after a later committed response
any active state -> Failed on terminal/non-retryable service failure
any active state -> Stopped on ordered shutdown
```

`Running` from the SDK starts task flags but does not prove connectivity. The
first new SDK response-commit observation for the current Core generation:

1. sets `connectivity_proven = true`;
2. advances `committed_generation` and the actor-private response sequence;
3. records `pos_present` without retaining or exporting the position;
4. changes Starting/Reconnecting to Running; and
5. wakes room-list and timeline reconciliation for that exact generation.

Offline or Error before the first commit stays retryable Starting/Reconnecting;
it never selects another engine. Offline or Error after connectivity moves to
Reconnecting and retains cache/timelines. Restart and reconnect reuse the
single engine contract with a new Core generation and reject delayed observer
messages from older generations.

`SyncBackendKind`, `SyncMode`, and the backend field on sync status/events are
removed rather than reduced to one-variant public types. Diagnostics and QA
render the fixed engine token `SyncService`.

## Room-List Ownership And Readiness

The one unfiltered `all_rooms` list is the sole live membership and ordering
source. Joined rooms, spaces, DMs, and invites are normalized from its entry
stream and the membership state of those exact entries. Core does not rebuild
the live list from `Client::rooms()` or `Client::invited_rooms()`.

The client store remains a cache source only. At cold start, cached rows can be
shown immediately with `source=cache`, `loading`, and stale presentation. They
are not authoritative evidence of current membership.

Room-list readiness becomes:

```rust
pub enum RoomListSource { Cache, Live }
pub enum RoomListLoadingState {
    Uninitialized,
    Loading,
    Ready,
    Failed { kind: RoomListFailureKind },
}
```

All transitions carry the Core sync generation. A live projection becomes
`Ready` only when:

1. at least one successful response commit exists for the generation;
2. the `all_rooms` list reports its current complete loaded range; and
3. `RoomActor` has reconciled the matching entries projection.

Before all three conditions, loaded live entries may update or augment cached
rows, but absence from a selective/growing range cannot remove a cached room or
prove leave. An authoritative zero-room result is accepted only after complete
range reconciliation. Delayed list diffs, completion signals, and room updates
from retired generations are ignored.

Refresh re-subscribes/re-normalizes the same service. It does not call
`/v3/sync`, create another `RoomListService`, or enumerate client rooms as a
second live truth path. The current auxiliary post-commit room-update wake may
remain only as a bounded wake for membership/profile changes of IDs already
owned by `all_rooms`; it cannot add or remove list membership itself.

## Timeline And Gap Repair

All room, thread, and focused timelines remain SDK event-cache-backed. Opening
a room subscribes it through the mandatory live `RoomListService`, preserving
the Element X room-open pattern.

Legacy timeline provenance and fallback checkpoints are deleted. The SDK
observable introduced for response commits supplies a generation-fenced global
commit checkpoint after event-cache mutation. Existing room-subscription
checkpoints remain actor-private evidence for responses containing an active
room. Core gap repair uses:

- Core sync generation;
- SDK response-commit sequence;
- the active room-subscription generation/checkpoint when present; and
- an authoritative event-cache continuity inspection after that commit.

If an active room is omitted from one incremental Sliding Sync response, that
omission is not leave evidence. Only the committed global checkpoint permits a
bounded event-cache reinspection for the newest live-edge gap; it does not
permit arbitrary history deletion or list-membership removal. Reconnect keeps
the same persisted event cache, rejects stale generations, and relies on SDK
deduplication so visible events are not duplicated or reordered.

`MatrixCommittedRoomTimelineBackend` is removed. Any remaining provenance type
is engine-neutral and can represent only the mandatory Sliding Sync generation
and event-cache observation.

## User-Copyable Diagnostics

Diagnostics remain a dedicated non-product-state lane. `SyncActor`,
`RoomActor`, and `AccountActor` publish fixed-shape typed diagnostic slices to
an AccountActor-owned latest-wins snapshot. `CoreRuntime` exposes that snapshot
to the existing diagnostics command; React does not derive sync semantics from
logs or room rows.

The existing Diagnostics dialog appends a `Sliding Sync` section and includes
it in the existing Copy diagnostics action. The Rust formatter emits only the
following fixed keys:

```text
sliding_sync.discovery_state
sliding_sync.advertised
sliding_sync.discovery_source
sliding_sync.last_probe_age_bucket
sliding_sync.last_http_status_class
sliding_sync.request_schema=element_x_all_rooms
sync.engine=SyncService
sync.lifecycle
sync.connectivity_proven
sync.committed_generation
sync.last_success_age_bucket
sync.consecutive_failure_count
sync.last_failure_kind
sync.room_list_task_running
sync.encryption_task_running
sync.pos_present
room_list.source
room_list.loading_state
room_list.cache_count
room_list.live_count
room_list.invited_count
room_list.initial_live_commit_seen
room_list.last_projection_age_bucket
room_list.subscription_count
room_list.reconciliation_pending
```

Age buckets are fixed (`never`, `<1m`, `1-5m`, `5-30m`, `30m-2h`, `>=2h`).
Failure and lifecycle values are allow-listed enums. Counts are saturating
non-negative scalars. No free-form field is accepted by the formatter.

Contract tests serialize and copy a maximally populated synthetic snapshot and
assert that tokens, position values, Matrix/user/device/room/event IDs,
aliases, URLs, names, message/media content, local paths, raw errors, and
response bodies cannot appear. A second inventory test rejects newly added
string fields that are not fixed enums.

## Local QA And CI

### Positive Matrix

Tuwunel is the primary fast lane. Synapse is the second required lane and is
pinned to a current stable image. Generated Synapse configuration explicitly
sets `msc3575_enabled: true`, even when upstream defaults it on.

Both servers run the same `sliding_sync` integrated scenario and assert through
`CoreEvent`/snapshot state:

- advertised capability and fixed SyncService engine;
- first committed response connectivity;
- joined room, room invite, and space invite through `all_rooms`;
- invite accept and decline;
- DM start and projection;
- encrypted receive;
- persisted-cache restart from stale to live;
- network reconnect without engine replacement; and
- coherent room list, timeline, and gap-repair state.

The enforced output is token-only:

```text
sync_backend_a=SyncService
sliding_sync_capability=ok
sliding_sync_committed=ok
invite_recv=ok
invite_accept=ok
invite_decline=ok
dm_start=ok
diagnostics_redaction=ok
```

### Negative Matrix

A Synapse fixture with `msc3575_enabled: false` proves the pre-login
`Unsupported` transition and no credential/store destruction. Generic local
HTTP fixtures prove absent/false capability, malformed response, timeout,
DNS/TLS/transport classes, stale completions, and retry. They are protocol
fixtures, not Conduit fingerprints.

### Conduit Removal

Conduit is removed from supported server arguments, Docker/config launchers,
positive CI, package scripts, docs, and troubleshooting commitments. No deny-
list is added. A Conduit deployment that advertises the protocol may receive
the standard request, but it is untested and unsupported.

`core-backend=legacy`, `core-backend=both`,
`KOUSHI_QA_FORCE_SYNC_BACKEND`, and equivalents are removed. Supported local
server values become `tuwunel`, `synapse`, and `both` where `both` means those
two positive targets.

## TDD And Verification

The implementation order follows issue #412 exactly. Legacy deletion is gated
by a green Tuwunel invitation lane and a green Synapse invitation lane.

1. Add failing capability, state, request-parity, diagnostics, and no-Legacy
   inventory tests.
2. Make both positive invitation lanes green without deleting Legacy.
3. Freeze the serialized Element X all_rooms request contract.
4. Remove the invite-only probe.
5. Implement capability admission and stored-session blocking/retry.
6. Replace provisional classic sync with restricted Simplified Sliding Sync.
7. Make SyncService and RoomListService non-optional.
8. Remove Legacy runtime, fallback, types, refresh paths, and QA switches.
9. Remove Conduit surfaces.
10. Wire copied diagnostics and privacy tests.
11. Run focused unit/integration checks, then one integrated local-server run,
    frontend lint/typecheck/tests, Rust formatting, workspace tests, and CI.
12. Self-review every tracked and untracked diff against the amended canon.

A final repository inventory allows `LegacySync` only in explicitly historical
dated records marked superseded. Production Rust, DTOs, TypeScript, fixtures,
scripts, active docs, and QA commands must contain none.

## Failure And Rollback Policy

No PR may restore a hidden runtime fallback. Before Legacy deletion, a failed
positive lane blocks the migration and is fixed on the mandatory SyncService
path. After deletion, capability or sync failures retain credentials, stores,
cache, and retry actions; they do not switch engines.

If the parity guard finds a request semantic that the vendored SDK cannot
produce through its public or existing narrow patched API, implementation
stops before Legacy deletion, opens a focused SDK port/rebase prerequisite,
and resumes only when that guard is green. A broad SDK update is not folded
silently into #412.

## Completion Criteria

Issue #412 is complete only when all three PRs are merged and current `main`
proves every acceptance criterion from the issue. Green focused tests or one
merged phase are partial evidence, not completion. The final audit records the
exact tests, local-server runs, CI checks, source inventories, docs, and merged
commit that prove each requirement.
