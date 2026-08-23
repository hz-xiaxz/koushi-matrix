# Issue #645 Composer Load Session Fence

## Contract and current evidence

Issue #645 requires:

1. deterministically reproduce the flaky composer-load assertion under bounded
   concurrent/workspace-like load without weakening its exact contract;
2. trace session/generation ownership around corrupt load, repair, lock/unlock,
   and retry scheduling;
3. prove one Ready lifecycle cannot schedule an additional corrupt load;
4. preserve persistence, debounce, repair, and account switching;
5. use no sleep, larger assertion bound, or timing-based stabilization.

`AppActor::load_composer_drafts_for_current_session` currently owns a plausible
product fence:

- no Ready account key stores `ComposerDraftLoadStatus::Unloaded`;
- the same Ready `SessionKeyId` returns immediately from both `Loaded(key)` and
  `Failed(key)`;
- canonical lock/account replacement removes the key, after which a later Ready
  lifecycle may intentionally retry;
- the actor awaits the blocking load before processing another action batch.

Historical workspace runs observed 3 global `core.composer_draft/load_failed`
records where the test expected 2, while immediate exact/file/workspace reruns
were green. The repository does **not** identify a same-process foreign producer:
the corrupt tests are serialized and Cargo test binaries have separate process
globals. Therefore the historical extra attempt's root cause remains unproven.
Do not claim diagnostics contamination or product exoneration from source alone.

## Evidence correction

Add attribution directly to the existing test-only I/O probe:

- create one `Arc<AtomicUsize>` when
  `install_composer_draft_io_barrier_for_testing` installs a probe;
- retain that Arc in `ComposerDraftIoProbe` and
  `ComposerDraftIoBarrierForTesting`;
- increment it at every `StoreActor::load_composer_drafts` start before consuming
  the existing one-shot sender;
- expose `load_attempt_count()` with Release increment / Acquire read.

One installed probe owns one runtime/store-clone observation interval. Replacing
the probe freezes the old counter and starts a new zero counter. The counter is
behind `cfg(any(test, feature = "test-hooks"))` and carries no account, path,
draft, or error data.

## Verify first RED: ownership attribution under real concurrency

Refactor fixture setup into two phases without changing product behavior:

1. prepare a temporary data/credential store, seed a valid payload, corrupt it,
   but do not start CoreRuntime;
2. start the runtime, install its probe, inject the Ready actions, await the
   store load-completion one-shot, **then await that runtime's `wait_for_ready_room`
   state event**. The Ready-room publication is the actor-side settle barrier
   after `load_failed` is recorded; the store load-completion one-shot alone is
   not sufficient for a shared diagnostics-count read.

For `concurrent_corrupt_runtime_evidence_is_owner_scoped`, prepare **two**
independent corrupt stores first under the existing test locks. Sample one global
`core.composer_draft/load_failed` baseline before either runtime starts. Then
start both actual runtimes concurrently with `tokio::join!`. For each runtime,
await both its store probe and its Ready-room state event; only after both
Ready-room waiters settle may the test read the shared diagnostics count.

Before adding owner-local counters, remove/disable the fixture's internal
`failed_before + 1` assertion for this scenario and assert the shared global
count is `baseline + 1`, as if it identified one tested owner. This is
schedule-independent RED: both settled runtimes emit exactly one matching record,
so the actual delta is 2.

```bash
cargo test -p koushi-core --test runtime_timeline \
  concurrent_corrupt_runtime_evidence_is_owner_scoped -- --exact --nocapture
```

Record the gate's own non-zero exit and an assertion message containing the
baseline, both settled-runtime state, and exact expected-1/actual-2 delta. This
RED demonstrates that current global evidence cannot attribute attempts under
real concurrent runtime load; it does **not** claim this mechanism caused the
historical workspace +1.

After adding probe-local counters, retain the same preparation/start/join
scenario. Remove the obsolete per-fixture global assertion permanently and
assert both owner-local counters are exactly `[1, 1]` and the shared global delta
is exactly 2. The unchanged focused command must turn GREEN regardless of runtime
completion order.

## Same-session bounded stress

Retain the first installed barrier in `CorruptComposerLoadFixture` as an explicit
`failed_load_probe` field. After its first corrupt load completes:

- assert first probe count is exactly 1;
- install a second `unexpected_reload_probe`, whose initial count is 0;
- define `SAME_SESSION_LOAD_STRESS_UPDATES: usize = 64`; 64 is a deliberately
  small power-of-two stress sample that crosses many actor-loop iterations while
  remaining far below mailbox capacity and keeping the focused gate cheap;
- inject exactly that many benign same-session `RoomListUpdated` action batches,
  with the final batch containing a unique sentinel room;
- await the sentinel room through the authoritative state-event waiter;
- assert `unexpected_reload_probe.load_attempt_count() == 0` and its existing
  `load_started_before_release()` is false;
- assert the frozen first probe remains 1, closing the interval between its
  installation and second-probe replacement; the second probe is the detector
  for the complete settled burst interval.

This proves the same Ready lifecycle schedules no second/third load under a
bounded action burst. No sleep or relative-time success condition is allowed.

## Intentional lifecycle retry

In `lock_unlock_retries_repaired_composer_payload`:

1. retain the first corrupt probe and assert count 1;
2. install the repaired payload;
3. drive canonical lock, which causes the no-session loader call to reset status
   to Unloaded;
4. install a new repaired-lifecycle probe with count 0;
5. drive authoritative Ready restoration and wait for load completion;
6. assert repaired probe count exactly 1 and loaded document correctness.

No load-capable interval exists between the first failed attempt and lock reset:
`Failed(same key)` fences every Ready action. The two retained probe results
therefore prove exactly two intentional lifecycle attempts: corrupt 1 + repaired
retry 1.

## Workspace evidence gate

The focused actor shape cannot by itself reproduce every historical scheduler
condition. Product exoneration requires the probe assertions to execute in the
actual workspace modality:

```bash
for run in 1 2 3 4 5; do
  cargo test --workspace --all-targets \
    --exclude koushi-backend --exclude sidebar-composition --exclude key-management \
    || exit 1
done
```

The local exact-count assertions stay active in every run. Record all five run
results and CI. If any probe count exceeds the expected interval count, stop:
the product session fence is not exonerated, capture the exact state/action
sequence, amend this design, and obtain another pre-implementation verdict
before changing product logic. A focused green burst alone is insufficient to
close the issue.

## Minimal implementation when all local counts are correct

- `crates/koushi-core/src/runtime.rs` — barrier Arc creation/accessor;
- `crates/koushi-core/src/store.rs` — probe counter field;
- `crates/koushi-core/src/store/composer_drafts.rs` — increment at load start;
- `crates/koushi-core/tests/runtime_timeline.rs` — retained probe fields,
  concurrent two-runtime RED/GREEN, 64-action burst, lifecycle retry counts;
- this plan and `docs/agents/plans.md` — review/worklog index.

No product `ComposerDraftLoadStatus`, session/generation, persistence, command,
event, DTO, diagnostic, or storage format changes are approved by this plan.

## Privacy and concurrency

- Counters and one-shots contain only counts/settlement, never Matrix IDs,
  account identity, paths, bodies, revisions, or raw errors.
- Every matching diagnostic privacy assertion remains, but diagnostics count is
  no longer used as per-runtime attribution.
- Increment and take the existing `load_started` one-shot while holding the same
  probe mutex guard, then send after releasing the guard. Load-completion/state-
  event waiters settle observed work before Acquire reads.
- Probe replacement occurs under the existing mutex. Old handles remain frozen;
  no global reset race is introduced.
- The two-runtime RED uses real StoreActor loads and separate temporary stores,
  not a fabricated diagnostic event.

## Preservation and full gates

Keep existing revision-fail-closed, corrupt-payload-unchanged, repaired-load,
account-switch ordering, persistence, debounce, and lease tests. After focused
GREEN run the complete `runtime_timeline` test, five workspace/all-targets runs,
Tauri, state/SDK/core, wasm, QA binary, frontend matrix, SDK guard, rustfmt,
docs/generated/boundary/security/dependency gates, relevant homeserver lanes,
`git diff --check`, and CI 7/7.

## Non-goals

- No weakened count, sleep, larger timeout/assertion, retry, new production
  generation, product lock, diagnostics suppression, or speculative fallback.
- No claim that concurrent in-process attribution was the historical root cause.
- No issue closure if any workspace-local probe count violates the invariant.

## Acceptance mapping

| Requirement | Evidence |
| --- | --- |
| deterministic concurrent reproduction | two real corrupt runtimes expose global attribution RED |
| one session cannot schedule another attempt | second probe remains exactly 0 through 64 settled same-session actions |
| intentional repaired retry is exact | first probe 1 + post-lock probe 1 |
| workspace scheduler modality covered | five complete workspace/all-targets runs with local assertions active |
| assertion not weakened | exact owner-local counts replace ambiguous global attribution; privacy shape remains |
| session/generation trace | Failed(same key), no-session Unloaded reset, Ready retry documented and tested |
| persistence/debounce/repair/account switch preserved | existing file/full workspace matrix |
| private-data free | count-only probe and existing empty diagnostic-field assertions |

Any local-count failure returns to design review; the exact final diff and logs
require post-implementation review before PR creation.

## Design review record

- Round 1, `reviewer-flash-opencode-go`: `Not correct-to-merge`; removed the
  unsupported cross-binary contamination diagnosis, retained the historical
  root cause as unproven, and made workspace-local probe counts the closure gate.
- Round 2: `Not correct-to-merge`; replaced scheduler-dependent per-fixture
  sampling with one pre-start baseline and joined two-runtime settlement.
- Round 3: `Not correct-to-merge`; pinned shared-count reads after actor-side
  Ready-room settlement because store load completion precedes diagnostic record.
- Round 4: `Correct-to-merge`; deterministic RED/GREEN ordering, probe ownership,
  64-action stress, retry accounting, privacy, and workspace gates verified.

## Implementation evidence

- RED, before counter plumbing: the exact concurrent-runtime command exited 101
  with `expected_delta=1 actual_delta=2 baseline=0`, both runtimes settled, and
  0/1 tests passed.
- GREEN, unchanged command after owner-local counters: 1/1 passed. The exact
  same-session stress and repaired-lifecycle tests also passed 1/1 each; final
  decisive assertions are first probe 1 / replacement probe 0 through 64 settled
  updates, concurrent owners `[1, 1]`, and corrupt 1 + repaired retry 1.
