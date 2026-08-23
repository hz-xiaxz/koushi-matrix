# Issue #552 Frontend Ownership Inventory

## Deliverable

Publish `docs/architecture/frontend-ownership-inventory.md`, add its explicit link to `docs/architecture/overview.md`, and check in the pinned public issue snapshot at `docs/architecture/evidence/issue-552-contract.json`. This is classification only; it moves no owner and does not close #552.

## Evidence sources

- current `App.tsx`, `TimelineView.tsx`, `appStore`, `timelineStore`, Browser Fake and harness;
- #111 Rust StateDelta baseline;
- #550 lifecycle fixes and all #551 App/TimelineView/runtime/fake residual audits;
- merged #645/#657/#658/#668 contracts;
- #570/#582 active designs, marked separately so inventory does not claim unmerged behavior; #660 merged during final inventory review and is classified as shipped;
- pinned #552 contract (`https://github.com/shinaoka/koushi-matrix/issues/552`, checked-in JSON SHA-256 `0371538cb18ab90b399fbd8114ec0678603ef3d24797e3f70d182898910c268f`) plus freshly fetched/restated #659 late room-list projection, #608 UnknownToken/auth diagnostics, and #559 local-vs-server read scopes. The inventory maps disjointness/epic criteria and does not call the three restatements pinned artifacts.

Use path:symbol evidence, not line-count intuition.

## Classification rubric

Every row records:

1. current owner and exact site;
2. lifetime and whether owner can disappear while work remains;
3. classification:
   - Rust domain/product state,
   - frontend renderer/presentation state,
   - transport/projection cache,
   - test-backend mirror;
4. authoritative source and duplicate semantics, if any;
5. teardown/terminal settlement;
6. decision: keep, delete/derive, migrate leaf, or investigate;
7. priority and concrete property improved by migration.

A frontend ref is not a migration target merely because it is long-lived. DOM geometry, focus, hover, drafts before dispatch, and projection caches stay frontend-owned. Browser Fake is not product authority; its duplicate semantics are test-mirror debt, not a Rust migration.

## Required rows

At minimum classify:

- appStore StateDelta cache and generation-gap refresh;
- timelineStore keyed CoreEvent cache, indices, pagination/gap state;
- TimelineView mounted DOM/virtualization/anchors/measurement/backfill controller;
- projection/repair acknowledgement retry timers and signatures;
- room-key request optimistic pending Set/epoch/toast;
- avatar request/retry window;
- App latest-text async queue and request refs;
- composer overlays, renderer leases, submission registry and debounce timers;
- Space/room/directory/search request fences and search debounce;
- state-refresh timer and Core/Tauri listeners;
- QA refs/listeners/diagnostic generations;
- Browser Fake semantic maps (composer/upload/submission/activity/space members);
- harness prepared bytes/leases/history after #657;
- purely visual dialogs, widths, pointer listeners and focus timers.

## Required conclusions

- explicitly list already-correct Rust-owned/projection-only paths;
- list duplicated TS/Rust semantic transitions separately from intentional fake mirrors;
- identify owner-disappearance risk and settlement for each resource;
- rank at least three disjoint leaf candidates;
- recommend one Wave C leaf disjoint from #659/#608/#559 and #570 work;
- keep #552 open and restate every #552 acceptance criterion with shipped,
  active-design, inventory-only, or remaining status.

Required room-key evidence: cite `DecryptRetryController::admit`,
`begin_decrypt_retry`, `handle_request_room_key`, and TimelineActor
`key_request_states`. Rust already owns admission/coalescing and terminal state;
the frontend `pendingKeyRequests` Set owns only per-event optimistic presentation,
timeline-key/account-reset handling of delayed rejection, and dispatch suppression. Classify it as
presentation/investigate, not a default migration, unless a concrete semantic gap
is proven.

Default Wave C candidate: audit/retire App's `latestTextOperationQueueRef` and
`applyLatestTextSnapshot` if Rust composer revisions plus StateDelta/full-snapshot
generation admission already reject stale text results. Deleting a duplicate TS
sequencer after deterministic proof is an ownership migration to the existing
Rust authority, not a line-count move. It is disjoint from #659 (room-list
projection), #608 (auth diagnostics), #559 (read-state engine), and #570
(redaction/activity/thread).

Second candidate: projection/repair ACK retry scheduling. DOM evidence remains in
React; only semantic retry/backoff/terminal ownership may move after a reviewed
transport design.

## Verification

- source path/symbol links resolve;
- classifications agree with architecture/state-ownership docs and #551 residual verdicts;
- no active/unmerged design is presented as shipped;
- add an explicit `docs/architecture/overview.md` link to the inventory, then
  verify the link resolves;
- plan index links this plan;
- agent docs/diff checks;
- independent design/content review before commit and exact final diff review before PR.

## Non-goals

No code, DTO, command, state, timer, dependency, issue closure, or speculative architecture implementation. No reopening #111/#551 without concrete contradiction.

Implementation of the documentation begins only after `reviewer-flash-opencode-go` records `Correct-to-merge`; the final inventory diff requires post-review.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`. Required pinned
  #552/#659/#608/#559 contracts and falsifiable disjointness, Rust room-key
  admission/coalescing evidence and candidate re-scoping, explicit epic criterion
  mapping, and an overview-link deliverable. The default candidate is now the
  duplicate latest-text sequencer retirement, subject to proof.
- Round 2: `Correct-to-merge`. The rubric/rows, pinned contracts, room-key
  evidence, active-vs-shipped distinction, disjoint leaf ranking, epic mapping,
  and overview deliverable were verified against source.
