# Issue #634 Browser Fake Prepared-upload Lifecycle

## Status

Design revision 3 approved by `reviewer-flash`: `Correct-to-implement`. Round 1 added implicit thread-close paths; round 2 added public cross-kind minimality evidence and exact production empty/item-count/128-MiB admission.

## Problem and ownership

`preparedUploadBytes` is the fake's private variant-byte cache. It currently survives target replacement, thread close, room removal, and all session teardown paths; `stageUploadBytes` also admits every input while Rust `MediaPreparationCache::prepare_items` takes at most `MAX_PREPARATION_BATCH_SIZE = 16`.

The map stays on `BrowserFakeApi`; this PR fixes its existing target/room/session ownership without introducing another owner object or an arbitrary byte/count cap.

## Verify-first RED proofs

Use only public methods and `preparedUploadPreview`:

1. **Target replacement:** stage `old` bytes for active main target, replace with a disjoint `new` staged ID, require old preview empty and new present.
2. **Batch parity:** require empty,17-item, and over-128-MiB batches to reject with the production command error and leave projection/cache unchanged. The byte-limit test uses one sparse JavaScript array whose length exceeds the limit, so it does not allocate128 MiB of elements.
3. **Explicit thread close:** open thread, stage bytes, close it, require preview empty.
4. **Thread replacement:** open root A, stage bytes, directly open root B without explicit close, require A preview empty.
5. **Implicit thread close on room/navigation change:** stage thread bytes, call `selectRoom` and separately the public `selectSpace` path that reaches `clearActiveRoomSelection`, and require preview empty.
6. **Room removal:** stage active-main bytes, leave room, require preview empty.
7. **Session teardown table:** for fresh instances stage one main item, then independently call each existing clear owner (`completeOidcLogin`, failing `submitLogin`, `switchAccount`, `changeHomeserver`, `logout`, `resetLocalData`) and require preview empty.

All lifecycle/bound REDs fail on the current implementation. Add characterization assertions for currently successful `clearUploadStaging` and `sendPreparedUploads`: both must keep returning empty previews after their duplicated loops are replaced. Pin minimality with public cross-kind isolation: stage main and thread bytes in the same room; clearing/sending one target must preserve the other kind's preview. Multiple thread-root caches cannot coexist after the approved `openThread` cleanup, so exact-root prefix minimality is additionally checked statically; a broad thread-room delete is observationally equivalent only at the explicit invariant that no stale root survives opening another root.

## Implementation boundary

- Add one named private method `clearPreparedUploadBytes(target)` that encodes the exact target prefix, including thread root, and deletes only matching keys. It is used by target replacement, send, and explicit clear.
- Add one distinct private owner operation `clearPreparedThreadUploadBytesForRoom(roomId)` matching Rust's `clear_thread_targets_for_room`; it clears every thread-root target in that room and is used whenever the fake closes/replaces a thread pane.
- `stageUploadBytes`: before even checking whether the target is active, reject empty, more-than16-item, integer-overflow, and over-128-MiB batches with the same production command error. After that validation and the active-target guard succeed, clear the exact target before writing any new key, then process every admitted item.
- `stageUploads`: after admission, clear the corresponding main target before metadata replacement.
- `sendPreparedUploads` and `clearUploadStaging`: replace their duplicated broad room-prefix loops with exact-target cleanup.
- `openThread`: clear all prior thread targets for that room before installing the new root.
- `closeThread`, `selectRoom`, and `clearActiveRoomSelection`: capture the currently open thread room before replacing state and clear all thread targets for that room.
- `removeRoomFromFakeSnapshot`: delete the main prefix and all thread prefixes for that room.
- `clearSessionViews`: clear the entire map once.
- Add local constants mirroring Rust/Tauri's16-item and128-MiB command admission bounds; do not add any other total cache-byte, target-count, or lease cap.

Keep room switching behavior unchanged: cached bytes for another retained room are not discarded merely because it is temporarily inactive. Keep `nextComposerLeaseId`, composer/session fixes, variant construction, preview values, staged order/positions, and send acceptance unchanged.

Review found the parallel prepared-byte lifecycle defect in `appHarnessMain.tsx`. That separate #551 harness owner is recorded for its own verify-first task and is not hidden inside this browser-fake PR.

## Prefix and isolation contracts

- main target prefix: `main:<room>::`
- thread target prefix: `thread:<room>:<root>:`
- exact target cleanup must not clear another thread root or main target;
- thread-pane close/replacement clears every root for its exact room, matching Rust;
- room removal clears both prefix families for that room;
- session teardown clears all rooms/targets;
- colon-prefix matching is confined to the fake's authoritative room IDs, all of which include their full server suffix; no prefix is built from an unvalidated partial room ID.

## Verification and exactness

- every RED group observed before production change; exact tests GREEN three consecutive runs;
- browser fake/client full tests;
- invalid empty/17-item/over-128-MiB batches reject without projection/cache mutation; admitted positions/order remain exact;
- map writes remain only variant creation; deletes occur only through exact-target, thread-room, or room removal cleanup; map-wide clear only at session clear;
- public cross-kind isolation plus static prefix counts prove target cleanup cannot become map-wide; exact-root cleanup is protected by the no-stale-root-after-open invariant;
- all six `clearSessionViews` callers unchanged;
- public API/DTO/wire, fields/maps/timers/exports, variant IDs and bytes delta0;
- full frontend/Playwright/workspace/Tauri/Headless/wasm and policy/audit matrix;
- post-implementation full-diff review and CI7/7.

## Implementation evidence

- RED: disjoint replacement, invalid batch admission, explicit/implicit thread close, root replacement, room removal, and all six session teardown owners retained bytes or admitted invalid work on the baseline.
- Characterization pins successful send/clear cleanup and main/thread cross-kind isolation.
- Production: exact-target and thread-room owner cleanup operations; lifecycle sites updated; session map clear1; invalid empty/>16/>128-MiB admission; cache writes remain variant creation only.
- GREEN: prepared-upload group passed three consecutive runs; browser fake107 and client25 passed; typecheck and focused lint green.
- Post-implementation full-diff review after total-overflow simplification: `reviewer-flash` `Correct-to-merge`; full matrix pending.

## Delivery

One PR linked to #634. After merge, close #634 only after all six original findings are checked with evidence. Then resume the #551 pure settings seam and browser-fake residual audit.
