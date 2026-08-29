# Issue #750 canon and low-risk architecture cleanup

Status: implemented with focused verification; final-diff review and full PR gates pending.

## Outcome

Land one behavior-preserving cleanup PR that makes the current architecture canon internally consistent, deletes the unused `koushi-backend` crate, removes genuinely duplicated small Core helpers, and removes the remaining baseline flat-path timeline shims. This PR does not begin the larger crate splits in #763/#765 or transport changes in #759.

## Verified current boundary

Inventory is against `origin/main` at `59bd1032`.

- Production and QA use only Element X-compatible Simplified Sliding Sync. `docs/architecture/overview.md` still contains the retired MSC4186 probe, `LegacySync`, Conduit, forcing, and fallback contract in rule 10, runtime prose, and QA prose.
- `koushi-backend` has no consumer outside its own tests. Live references are workspace/CI/README/current architecture-policy documentation, plus a negative Tauri-boundary assertion in `apps/desktop/src/scripts/macGuiQa.test.ts`; historical plans/specs/worklogs also retain the name. Historical records and the useful negative assertion remain unchanged.
- `koushi-sdk` contains two `smoke`-feature direct-adapter binaries and private-data-free smoke report helpers. They exercise SDK primitives; they are not the product runtime or the authoritative Core QA lane. Both `overview.md` and `REPOSITORY_RULES.md` must state this narrow test-only exception instead of claiming no QA code exists in the crate. The existing default non-`--core` runner leg is adapter integration smoke, not evidence for product semantics.
- Four `current_epoch_ms` implementations have the same practical contract, but two clamp `u128` milliseconds to `u64::MAX` while two use a bare cast. The shared helper must use the clamped form.
- Two `atomic_replace_file` implementations share the same tempfile/write/sync/persist algorithm, but the credential-vault copy additionally creates and syncs the parent directory. The shared helper will preserve the stronger create-and-directory-sync behavior for both callers.
- Two `classify_report_error` implementations are identical.
- Two command-to-state `SearchScope` mappers are identical.
- The three functions named `classify_http_error` are not duplicates: they classify different domain results and must remain separate.
- Marked baseline flat-path shims remain in `timeline.rs` and `room.rs`; `account.rs` has none. In `timeline.rs`, external live uses of the marked shim set are the scheduled-send/runtime composer calls; other marked exports are unused or already use child-module paths. In `room.rs`, the marked `EncryptionDebugTestControl` and `classify_room_error` exports are unused. Intentional façade exports without the baseline/unused marker (actor/manager/residency/read-persistence/navigation types, `RoomOperationKind`, and public API types) are not compatibility shims and remain.

## Changes

### 1. Reconcile canon and current documentation

- Replace architecture rule 10 (`overview.md` current lines 801–833) with the single Simplified Sliding Sync contract, and integrate the trailing “Current sync contract” note into that normative section.
- Delete retired backend-selection claims in the QA model/current workflow and Conduit-specific selection prose. Re-attribute—do not delete—the still-live all-rooms response commit fence, generation fencing, presence limitation, canonical cursor, and E2EE QA passages currently carrying `LegacySync`/probe wording (current lines around 299–300, 405–406, 1318, 1348–1349, and 1486–1487). The live global response-commit fence remains part of gap repair under Simplified Sliding Sync.
- Reconcile the maintained normative `docs/architecture/state-machine.md`: delete the obsolete Sync Mode/backend-selection section around current lines 478–510 and the MSC4186/Legacy invite-selection contract; change room-list source/readiness prose to the current `Cache|Live` and single-service model; re-attribute generation replacement, invite QA, room-entry gap-repair provenance/response sequencing, and canonical-cursor passages to Simplified Sliding Sync without deleting their live guards. Update every current occurrence, including the passages around current lines 554, 831–880, 1647, and 3444, and remove/fix the trailing note that incorrectly points to older diagrams “below.”
- Update current QA prose to Tuwunel/Synapse only.
- Define `koushi-sdk` in both `overview.md` and `REPOSITORY_RULES.md` as the low-level SDK adapter that may include feature-gated, direct-adapter smoke binaries/reports; product state, actor lifecycle, and authoritative app QA orchestration remain in Core.
- Remove current `koushi-backend` responsibility/test instructions from `REPOSITORY_RULES.md`, `engineering-rules.md`, README, and normative `overview.md`. `docs/architecture/desktop-foundation.md` and `docs/architecture/tauri-react-shell.md` are dated and already labeled historical foundation, so preserve their references as history. The remaining architecture documents (`i18n.md`, `search-adapter.md`, `frontend-ownership-inventory.md`, and `evidence/`) contain no current `koushi-backend` responsibility and remain unchanged. Historical dated plans/specs/worklogs remain historical evidence.

### 2. Delete `koushi-backend`

- Remove the crate directory, workspace member, obsolete CI exclusion, and lockfile package entries.
- Keep the existing Tauri adapter boundary and ESLint import guards unchanged; they enforce other live boundaries and need no backend-specific replacement.

### 3. Deduplicate only proven equivalents

- Add a crate-private clock helper and route the four epoch-millisecond users through it.
- Add one crate-private atomic replacement helper returning `io::Result`, always creating the parent and syncing file plus parent directory. Callers retain their public/domain error mapping and fail-before-persist test hook. Add focused coverage proving a store write creates a missing parent and that fail-before-persist preserves the prior file for both caller paths; no caller may depend on the old missing-parent failure.
- Add one SDK-report failure mapper outside protocol DTO modules and use it from Account/Room.
- Put command-to-state scope conversion on the Core `SearchScope` type and use it from routing/search.
- Leave the three domain-specific HTTP classifiers separate.

### 4. Remove flat-path shims

- Make only `timeline::composer` crate-visible, update the scheduled-send/runtime callers of the marked composer shim to that owning module, and delete the unused marked composer/item-projection re-exports and allow attributes.
- Delete the unused marked `room.rs` exports (`EncryptionDebugTestControl` and `classify_room_error`) and their allow attributes. Do not remove the live `RoomOperationKind`/`RoomOperationTestControl` exports.
- Retain intentional façade/public API re-exports such as actor/manager/residency/read-persistence/navigation types and `sdk_item_to_timeline_item`; they are live module APIs, not the baseline compatibility shims targeted by this issue.

## Verification

Before implementation, capture baseline collection for the affected packages. After implementation require fresh evidence from:

- `cargo metadata --no-deps` and a one-shot scoped grep audit proving `koushi-backend` is absent as a workspace package/current responsibility and retired sync-selection claims are absent from current canon/operational docs. The negative assertion in `macGuiQa.test.ts` and historical records may legitimately retain the searchable name. This is review evidence, not a new global vocabulary lint: re-attributed live identifiers may also retain searchable terms, while `scripts/check-agents-docs.mjs` remains the durable operational-doc enforcement;
- `node scripts/check-agents-docs.mjs` and documentation/lint structural checks;
- `cargo fmt --all -- --check`;
- focused `cargo test -p koushi-core --lib`, `cargo test -p koushi-sdk`, `cargo check -p koushi-sdk --features smoke --bins`, and affected Tauri boundary tests;
- the repository CI-equivalent Rust workspace command, Core QA binary tests, frontend typecheck/lint/tests/build, browser headless, wasm, secret scan, cargo-deny, and cargo-machete as required by `.github/workflows/ci.yml`;
- exact final diff self-review and different-model final-diff verdict.

Local homeserver or native GUI behavior is not changed; those lanes may be satisfied by the exact-head hosted CI checks unless a focused gate reveals behavioral impact.

## Review gate

- Pre-implementation reviewer round 1: `reviewer-flash` (different model family from Luna), `VERDICT: FINDINGS`. Required corrections: distinguish deleted sync-selection prose from re-attributed live commit-fence behavior; correct the timeline/room marked-shim inventory; amend both normative SDK-responsibility statements; enumerate undated architecture docs; compile smoke-feature bins.
- Pre-implementation reviewer round 2: `reviewer-flash`, `VERDICT: FINDINGS`. Required corrections: include normative `state-machine.md` in the delete-vs-reattribute sync cleanup; distinguish already-labeled historical architecture docs; pin clamped epoch semantics; test strengthened atomic-parent behavior; remove an inaccurate backend-boundary-test claim.
- Pre-implementation reviewer round 3: `reviewer-flash`, `VERDICT: FINDINGS`. Required correction: delete the obsolete normative Sync Mode/backend-selection diagram and fix its trailing historical pointer; scope the backend grep audit around the intentional negative Tauri assertion.
- Pre-implementation reviewer round 4: `reviewer-flash`, `VERDICT: CORRECT-TO-IMPLEMENT`. Minor implementation reminder: remove the retired Conduit/backend QA command at the current `state-machine.md` line 3534 under the already-approved QA prose scope.
- Pre-implementation verdict: **approved**.
- Implementer: `luna-implementer` after the round-4 approval; two bounded Luna slices produced the implementation draft, and the main agent completed integration after their timeout checkpoints.
- Final-diff reviewer: pending (`reviewer-flash`).
- Final integration/self-review: pending (`gpt-5.6-sol`).

## Implementation evidence

- `cargo check -p koushi-core`: passed.
- `cargo test -p koushi-core --lib`: 1085 passed, 8 ignored.
- Focused credential-vault atomic tests: 6 passed; composer missing-parent and fail-before-persist tests: passed.
- `cargo test -p koushi-sdk`: passed.
- `cargo check -p koushi-sdk --features smoke --bins`: passed.
- `cargo metadata --no-deps`: passed with no `koushi-backend` workspace member.
- Full repository/PR gate evidence remains pending after final-diff review.

## Acceptance

- Canon describes one current sync/runtime model and the narrow SDK smoke exception without weakening Rust ownership.
- `koushi-backend` and all live workspace/current-doc references are gone without replacement abstraction.
- One authoritative implementation remains for each proven duplicate; non-equivalent HTTP classifiers remain local.
- Marked baseline flat-path shims are absent from timeline/room modules, while intentional live façade exports remain.
- No product behavior, security/privacy contract, QA scenario/token, or public protocol changes.
