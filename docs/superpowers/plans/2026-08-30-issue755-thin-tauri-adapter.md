# Issue #755 thin Tauri adapter and Core-owned settlement

Status: design approved for implementation by different-model review round 3.

## Outcome

Land one headless-first ownership migration PR that leaves `apps/desktop/src-tauri` responsible only for IPC decoding/encoding, Core command submission, event/snapshot forwarding, native path/capability acquisition, and native effect invocation. Core owns request correlation/settlement, staged-upload product policy, media-save safety, composer generation/lease validity, and secure-backup confirmation admission. React owns confirmation/dialog presentation and mounted editor/DOM state only.

This PR changes ownership, not Matrix behavior or visible upload/save/backup semantics. It builds on #738's committed room-selection settlement and does not redesign #759 state transport or #760 renderer acknowledgements.

## Verified baseline

Inventory is against `origin/main` at `a03aeb86` after #753.

- `CoreConnection::select_room_and_wait` already proves one Core-owned snapshot/event settlement path with typed failures and lag recovery.
- src-tauri still contains 25 `wait_for_*` functions. Twelve are concentrated in session/navigation/timeline/local-encryption; directory/room/search contain the same product correlation pattern. Three test-only event-source traits (`SelectEventSource`, `SubmissionEventSource`, `InviteWorkflowSnapshotSource`) preserve adapter-owned state machines. Exactly one waiter, diagnostics `wait_for_qa_recovery_prompt`, is diagnostic-only and remains outside product settlement.
- src-tauri `stage_upload_bytes` and its related selection/retry/replacement path own the 128 MiB batch bound, MIME normalization/classification, Preparing/Ready publication, account/target fences, blocking preparation/encoding, and prepared-registry merge.
- src-tauri owns media-save filename sanitization, absolute-path checks, cache-root canonical containment, destination creation, and copy.
- Core's `ComposerDraftLeaseRegistry` already allocates and validates renderer generations, leases, scopes, and permits. `ComposerDraftTransportIdentities` adds a second pair of counters/maps only to translate opaque strings back to the Core newtypes.
- Secure Backup already has Rust enforcement: `SecureBackupGateState::ExplicitlyDisabledRequiresSetup` and SDK `SecureBackupReenableConfirmationRequired`. The adapter nevertheless owns the native warning decision/copy and chooses the boolean that routes Core to setup versus re-enable.

## Canon-first amendments

Before behavior moves, amend `docs/architecture/overview.md`, `docs/architecture/state-machine.md`, and the relevant ownership inventory:

- request completion is a Core connection service: one absolute deadline, initial/final authoritative snapshot checks, typed RequestId correlation, and lag recovery;
- media staging policy and prepared-byte ownership are Core runtime responsibilities; IPC only supplies bytes/typed user choices;
- cache containment, destination validation, and safe-filename policy are Core security controls executed over a Tauri/platform filesystem port; adapters supply roots/dialog paths and syscalls but do not choose admission;
- composer wire strings are parsed into Core identities and validated by the Core registry; no adapter registry/counter exists;
- secure-backup re-enable requires Core admission plus explicit user confirmation; React/Tauri may render a dialog but cannot decide whether confirmation is required.

Reducer transition changes amend state-machine diagrams in the same change. User-visible confirmation copy uses the existing message catalog and pseudo-locale coverage.

## Phase A — one Core request-outcome service

### Contract

Add a non-serde Core API beside `CoreConnection`, not a new broadcast protocol:

```rust
pub enum OutcomeCorrelation {
    Request(RequestId),
    Submission { request_id: RequestId, submission_id: SubmissionId },
}
pub enum RequestOutcomeExpectation { /* closed typed variants */ }
pub enum RequestOutcome { /* typed payload/snapshot variants */ }
pub enum RequestOutcomeError {
    OperationFailed { failure: CoreFailure },
    FailedNoOp { reason: IntentNoOpReason },
    Lagged,
    Disconnected,
    TimedOut,
    InvalidOutcome,
}

impl CoreConnection {
    pub async fn wait_for_request_outcome(
        &mut self,
        correlation: OutcomeCorrelation,
        expectation: RequestOutcomeExpectation,
        baseline_generation: u64,
        deadline: Instant,
    ) -> Result<RequestOutcome, RequestOutcomeError>;
}
```

The submitting connection still owns `RequestId` creation and `CoreCommandHandle::validate_request_id` remains the sole foreign-connection forgery boundary. A separate attached event/snapshot connection may wait on the globally unique full RequestId; it must not compare the ID to its own `connection_id`. RequestId never comes from WebView input. Submission outcomes carry both the submitting RequestId and WebView-visible `SubmissionId`; neither value alone may settle the other request.

The expectation enum is closed and feature/domain named, not a callback/regex/generic retry framework. A new waiter requires a new closed variant, explicit account/target guards, lag policy, RED tests, and review; arbitrary predicates/callbacks are forbidden.

### Existing-waiter migration matrix

| Adapter waiter(s) | Correlation and authoritative success guard | Lag policy |
| --- | --- | --- |
| OIDC authorization | login RequestId + matching authorization event | terminal `Lagged` after final state check |
| logged-in/auth-changed | login/restore RequestId + baseline generation + exact account/session state; fold `wait_for_auth_changed` into this composite expectation | continue from watch snapshot |
| local-data reset | reset RequestId + `SignedOut` snapshot newer than baseline | continue from watch snapshot |
| focused closed/open/main anchor | command RequestId + account/timeline key + exact focused/anchor state newer than baseline | continue from watch snapshot |
| search started/closed | search RequestId + query/scope/request state | terminal `Lagged` after final state check |
| room/space created | command RequestId + exact created ID event and resulting known-room/space state | terminal `Lagged` if payload cannot be recovered |
| DM started + room-in-state | one composite start-DM expectation: RequestId event yields room ID, then the same account's room projection must contain that ID at a newer generation; delete standalone uncorrelated waiter | continue from watch after ID is known |
| room joined | join RequestId + joined ID event + same-account known-room state | continue from watch after ID is known |
| invite workflow snapshot | originating open/mutation RequestId + account/destination/query generation captured at submit; no bare snapshot-only waiter | continue from watch snapshot |
| invite batch / member role / generic room operation | command RequestId + exact event kind/account/room/target and settled snapshot | continue for snapshot-backed outcomes; terminal `Lagged` for event-only payload |
| upload staging | command RequestId + account/ComposerTarget + exact staged IDs/state newer than baseline | continue from watch snapshot |
| composer acceptance | command RequestId + account/target + expected revision | terminal `Lagged` after final snapshot check |
| submission settlement/outcome | `Submission { request_id, submission_id }` + exact target/account + terminal submission event/state | terminal `Lagged` after final state check |
| prepared media queue | command RequestId + accepted transaction ID + exact `MediaSendQueued { request_id, transaction_id, key }`; this path has no SubmissionId | terminal `Lagged` after final state check |

`wait_for_submission_settlement` remains only a thin typed wrapper; `wait_for_auth_changed`, `wait_for_room_in_state`, and `wait_for_invite_workflow_snapshot_from` are absorbed into correlated composite expectations rather than modeled as uncorrelated public service calls. The sole diagnostic waiter retained in Tauri is `wait_for_qa_recovery_prompt`.

### Semantics

- Check the exact account/session/room/timeline/target guard and authoritative versioned snapshot before blocking. Snapshot-only success requires a generation newer than the captured baseline unless the expectation explicitly allows already-satisfied idempotence.
- Use one absolute deadline across all events and snapshot changes.
- Events classify typed early failure/payload; success requiring state visibility settles only when the corresponding versioned snapshot predicate is true.
- `IntentLifecycle` remains telemetry and never proves product success alone.
- Lag behavior is expectation-specific as in the matrix. Every terminal lag/closure/timeout performs one final authoritative snapshot check under the same deadline; `Lagged` is distinct from `Disconnected` and `TimedOut`.
- Unrelated requests/submissions/events are ignored; stale account/timeline/target identities and a matching room that appeared through another request cannot settle.
- Debug output exposes only expectation/outcome kinds, counts, booleans, RequestId and redacted placeholders.

Move all 24 production product waiter loops and all three fake event-source traits into Core connection/outcome tests. Tauri maps typed results/errors once and deletes duplicate product timeouts/traits. Keep `select_room_and_wait` as a thin typed wrapper over the service, preserving its public behavior. Update the stale `commands/mod.rs` comment: commands may delegate to Core high-level methods, but every accepted command still exits through the same Core command inbox/validation boundary.

### Verify-first evidence

RED Core tests cover event-before-snapshot, initial/idempotent snapshot success, baseline-generation fencing, unrelated/stale/foreign RequestIds, wrong account/target, forged WebView-supplied correlation rejection at command submission, submission-ID/request-ID mismatch, operation failure, benign/failed no-op, every lag policy, closure, timeout final check, and exact returned generation. For each migrated adapter waiter, focused Tauri tests prove delegation and absence of an adapter product loop/timeout.

## Phase B — Core-owned staged-upload orchestration

Add Core runtime/connection methods in a dedicated `media_staging` module that own the existing complete workflow:

- validate non-empty batch, max item count, checked total bytes, and the 128 MiB bound (named Core constant);
- normalize MIME and derive `StagedUploadKind`/initial compression state;
- publish Preparing through the Core command/state path and await its exact outcome using Phase A;
- run preparation/encoding through existing `crate::executor::spawn_blocking` on the native desktop path without holding the global media transition/registry lock; add no direct Tokio blocking call and no speculative wasm feature/backend in this PR (Core's documented future wasm work remains structural until a web spike);
- capture/revalidate account, target, staged IDs, selection generation, and current item residency;
- merge prepared source/variant ownership into `MediaPreparationService` only after revalidation; lock order is transition owner → registry, reverse acquisition is forbidden, and no await/encode occurs while either guard is held;
- replace/publish only the still-current item/selection and await Ready/Failed state;
- own caption derivation and item replacement semantics already represented by Rust DTOs;
- cancel/timeout/stale-target exits release unmerged prepared bytes and leave the current authoritative staged item unchanged.

Tauri retains only `StageUploadBytesInputItem` deserialization, converts it to the Core input DTO, invokes the Core method, and serializes the returned versioned snapshot. Selection, retry, caption, removal, clear, and send handlers delegate to typed Core methods where they currently manipulate/repair state or await it. File/dialog byte acquisition remains adapter/platform work.

Preserve the renderer's documented caption dirty-editor queue (#552 Phase 5B): it orders unacknowledged editor intents before Core admission and is not a second product-state owner.

RED headless tests cover batch limits/overflow, MIME classification, preparing→ready/failure, duplicate IDs, stale account, inactive/replaced target, item removal during preparation, selection generation races, retry, caption preservation, prepared-byte release, and exact snapshot generation.

## Phase C — Core media-save security policy over a platform port

Do not add direct filesystem syscalls to actor/product logic. Add a Core-owned policy service over a narrow platform port:

```rust
pub trait MediaSaveFilesystem: Send + Sync {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, MediaSaveIoError>;
    fn create_dir_all(&self, path: &Path) -> Result<(), MediaSaveIoError>;
    fn copy(&self, source: &Path, destination: &Path) -> Result<(), MediaSaveIoError>;
}

pub fn safe_media_save_filename(input: &str) -> String;
pub fn default_media_save_path(filename: &str, downloads: Option<&Path>) -> PathBuf;
pub fn save_downloaded_media<P: MediaSaveFilesystem>(
    port: &P,
    cache_root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<(), MediaSaveError>;
```

Core owns ordering and policy: absolute/URL checks, port-backed canonicalization, path-component containment (including symlink/prefix-sibling attacks), destination validation, selected-parent creation, and copy admission. The native `std::fs` implementation lives in the Tauri/platform adapter; tests use a deterministic fake port. Paths do not enter CoreCommand/CoreEvent/AppState, and no platform cfg or direct filesystem syscall is added to Core actor logic. Amend overview Platform Portability rule 3 to name this media-save adapter port alongside StoreActor-owned persistence ports.

Tauri resolves app-data/download roots and the dialog-selected destination, then passes them to the Core policy service; it cannot bypass the policy in production. No raw path appears in Debug/log/error. Windows-specific acceptance documents canonicalization/junction/short-name residual assumptions and relies on the hosted Windows gate; Linux tests cover symlink and component-prefix attacks.

RED tests cover empty/relative/URL source, missing cache, canonical source outside root, sibling-prefix and symlink escape, absolute destination requirement, forbidden filename characters, empty filename fallback, parent creation, successful byte copy, port failure classification, and private-safe errors.

## Phase D — remove the second composer identity registry

Expose canonical decimal wire conversion on Core-owned opaque identities:

- `ComposerRendererGeneration::{to_wire_string, parse_wire}`;
- `ComposerDraftLeaseId::{to_wire_string, parse_wire}`.

Parsing validates canonical nonzero bounded `u64` values but grants no authority. Add a Core connection method for lease acquisition that first revalidates the currently Ready account and active main/thread ComposerTarget, then asks `ComposerDraftLeaseRegistry` to validate live generation, exact scope, account, target, and permit kind. Tauri stores no generation/lease counter or map: remove `ComposerDraftTransportIdentities` and its mutex from `CoreRuntimeState`. Begin/acquire/release handlers convert strings only and call Core. Renderer/app startup must call begin-generation before any lease. Numeric generation values may restart in a new process; the guarantee is instead that a stale renderer cannot obtain a command permit without passing the new runtime's begin-generation, active account/target admission, and fresh lease acquisition. RED tests assert that contract rather than cross-process numeric uniqueness.

RED Core/Tauri tests cover canonical/invalid/zero/overflow tokens, retired generation, forged lease, generation/lease mismatch, release idempotence policy, cross-account and cross-target rejection, counter exhaustion, and renderer replacement. Browser fake and Playwright harness tokens currently shaped as `harness-composer-lease-N` migrate in the same slice to canonical numeric strings so transport parity remains exact.

## Phase E — Rust-owned secure-backup confirmation admission

Replace the boolean policy shape with closed typed intent/authorization:

```rust
pub enum SecureBackupSetupIntent {
    InitialSetup,
    Reenable { confirmed: bool },
}
```

Carry `SecureBackupSetupIntent` on both `AppAction::SecureBackupSetupRequested` and `SecureBackupSetupRequest` so the projected admission fact and actor command cannot diverge. Add BootstrapSecureBackup to the runtime's projection-acceptance routing gate: AppActor/reducer performs gate × intent admission before actor routing; invalid or stale intent emits a typed confirmation-required/failed-no-op result and produces no AccountActor effect. AccountActor maps only the admitted command intent to the SDK call, whose fresh inspection remains authoritative. Core rejects:

- re-enable-required state without explicit confirmation;
- stale/forged re-enable confirmation when the gate no longer permits that intent;
- initial setup that would reset/re-enable an existing explicitly disabled backup.

The SDK's fresh server/local/trust inspection is the authoritative confirmation guard. Core/state gate checks are admission and stale-intent rejection only; they must never override or weaken an SDK `SecureBackupReenableConfirmationRequired` result. `InitialSetup` maps to `explicit_reenable_confirmed=false`; `Reenable { confirmed: true }` is the only path mapping true. React renders the confirmation dialog from Rust-projected gate/requirement state using catalog text and sends the explicit confirmation fact. Tauri removes native hardcoded bilingual policy/copy and merely maps the typed command. Browser fake mirrors transport shape only.

RED reducer/Core tests cover every gate state, stale gate change between dialog and submit, cancel/no-command, confirmed re-enable, initial setup, duplicate confirmation, and typed private-safe failures. Browser tests cover visible dialog/cancel/confirm and exact IPC argument; no secret enters React state or screenshots.

State-machine impact is explicit: Phase A adds no reducer transition; Phase B reuses existing `SetUploadStaging`/selection/completion actions and changes only their runtime owner; Phase C/D add no AppState transition; Phase E changes `BootstrapSecureBackup` admission so `ExplicitlyDisabledRequiresSetup + InitialSetup|unconfirmed Reenable` remains in that gate with a typed confirmation-required failure, while confirmed Reenable may enter `CreatingBackup`. Amend that diagram/narrative before code.

## Change organization

One PR is retained because the user explicitly chose one PR per #749 child and #755 consolidated #756–#758; no slice is merged separately. The mandatory pre-implementation design gate is run once for this approved design and is not restarted for continuations. Every slice below nevertheless remains independently build/test green and receives an additional different-model integration checkpoint before the next starts; the final integrated diff receives the recorded merge verdict and self-review. Worklog/PR body distinguish checkpoint feedback from the mandatory design/final verdicts:

1. canon + Phase A typed outcome service and waiter migration;
2. Phase B staged-upload orchestration;
3. Phase C media-save security move;
4. Phase D composer token registry deletion;
5. Phase E confirmation admission/UI presentation;
6. exact inventory, final review record, and full gates.

No generic retry/event framework, compatibility shim, duplicated old/new production path, timeout inflation, public secret/path diagnostics, or unrelated #759/#760 transport/ACK redesign. Delete superseded adapter tests/helpers in the same slice that makes Core authoritative.

## Verification

- Focused RED→GREEN Core/Tauri/state/frontend tests per phase.
- Core library/integration and QA binary tests; src-tauri tests; workspace tests.
- DTO/IPC/typecheck and generated contract artifacts where command shapes change.
- Full frontend Vitest/build/lint/IME/boundary/secret gates and Playwright.
- wasm state/search, cargo-deny, cargo-machete, rustfmt, docs/agents checks, SDK submodule check, diff self-review.
- Exact different-model review after every implementation slice, then final exact-tree self-review.
- Hosted macOS/Windows and Tuwunel/Synapse checks green before merge.

## Review gate

- Pre-implementation reviewer round 1: `deepseek-brainstormer`, `VERDICT: FINDINGS`. Blockers fixed in this revision: all production waiters now have a correlation/guard/lag matrix; wait connections no longer reject globally unique IDs created by the separate submitting connection; media-save policy uses a Core-owned service over an adapter filesystem port. Important fixes also specify media lock/executor semantics, Core active-target composer admission, SDK-authoritative backup inspection, per-slice review gates, all three event-source traits, state-machine changes, WebView RequestId prohibition, and Windows path assumptions.
- Pre-implementation reviewer round 2: `deepseek-brainstormer`, `VERDICT: FINDINGS`. Corrections incorporated: prepared-media correlation is RequestId+transaction ID (no SubmissionId); process-restarted numeric composer tokens are not claimed globally unique; native staging uses the existing executor without speculative wasm implementation; setup intent is carried into reducer admission before actor/SDK routing; slice checkpoints are distinguished from mandatory gate verdicts.
- Pre-implementation reviewer round 3: `deepseek-brainstormer`, `VERDICT: CORRECT-TO-IMPLEMENT`. Non-blocking clarifications incorporated before implementation: setup intent rides both projected action and command request through an explicit projection-acceptance routing gate; browser fake/harness composer tokens migrate to canonical numeric strings in Phase D.
- Pre-implementation verdict: **approved**.
- Implementer: `luna-implementer` only after approval, in sequential write slices.
- Final-diff reviewer: pending (different-model read-only).
- Final integration/self-review: pending (`gpt-5.6-sol`).

## Acceptance

- Tauri product handlers decode/encode, delegate, forward, and invoke platform capabilities only; no product event loop, retry, state repair, media policy, security policy, or composer authority remains.
- One Core outcome service owns RequestId settlement with typed failures, absolute deadlines, watch-backed visibility, and lag/final-snapshot recovery.
- Core owns all staged-upload decisions/fences and prepared-byte lifecycle; renderer caption queue remains presentation-intent ordering only.
- Core owns filename/path/cache containment and save-copy validation.
- Core registry is the only composer generation/lease authority; adapter counters/maps are absent.
- Core/state owns secure-backup confirmation requirement and stale-confirmation rejection; GUI renders/cancels/confirms only.
- Headless tests prove success/failure, stale/cross-account/target identities, disconnect/timeout, path attacks, and confirmation guards.
- Existing visible behavior, Matrix semantics, privacy, and exact-once/session isolation remain green across all gates.
