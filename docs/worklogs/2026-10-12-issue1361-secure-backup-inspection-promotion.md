# Secure Backup inspection admission across connectivity and promotion (2026-10-12)

Issue #1361: with two restored accounts, the second account often showed a
Secure Backup failure about 30 seconds after startup, and pressing Retry
succeeded immediately.

Canon consulted: the Secure Backup inspection rules in
`docs/architecture/state-machine.md` ("Nonessential secure-backup server
inspection runs only while the accepted sync projection is `Running`…") and the
root rules on root-cause fixes, verification-first, and state-machine
discipline.

## Cause

`start_secure_backup_inspection` defers and arms a bounded connectivity-wait
deadline while `sync_connectivity_proven` is false. A proven
`SyncConnectivityChanged` edge recorded the boolean, but its admission side
effects ran only inside the `if self.session_promoted` branch. When the proven
edge arrived before session promotion, the deferred request kept its deadline
and expired into `BlockedFailed{Timeout}` even though sync was running. Manual
retry re-entered admission against the now-true boolean and settled in about
240 ms. Repeated true notifications are dropped by the unchanged-value early
return, so nothing else replayed the missed edge.

## Change

- `crates/koushi-core/src/account/recovery_backup.rs`: extracted the proven
  edge's admission into `admit_secure_backup_for_proven_connectivity` and added
  `replay_secure_backup_admission_on_promotion`, which starts the pending
  inspection and retires its deadline when connectivity was proven before
  promotion. It replays an inspection rather than the reconnection monitor
  because cancellation resets the recovery epoch on every demotion, so no
  inspection can have settled between the retained edge and promotion.
- `crates/koushi-core/src/account/trust_gate.rs`: both session-promotion paths
  (`promote_recovered_session_runtime` and the verified-projection promotion)
  call the replay next to the Secure Backup observer start.
- `docs/architecture/state-machine.md`: records that admission does not depend
  on the ordering of the proven edge and session promotion.

Coalescing and generation fencing are unchanged: the replay clears the pending
flag first, and `start_secure_backup_inspection` still coalesces onto an
in-flight inspection and captures `trust_generation` for its completion fence.
Unproven connectivity keeps the deadline and its retryable failure.

## Verification

- New headless actor regression test
  `proven_connectivity_before_promotion_starts_the_pending_inspection_on_promotion`
  (RED before the fix at the "promotion must start the pending inspection"
  assertion; GREEN after).
- `cargo test -p koushi-core --lib`: 1522 passed, 0 failed, 10 ignored.
- `cargo test -p koushi-core --lib account::recovery_backup::tests`: 41 passed,
  including the existing unproven-deadline, coalescing, and
  proven-before-deadline cases.

## Pending manual checks

Native two-account startup on a real device (the original report) remains a
manual confirmation; the headless actor test is the correctness evidence.
