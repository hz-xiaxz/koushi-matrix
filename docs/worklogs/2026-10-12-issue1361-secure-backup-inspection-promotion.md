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

- `crates/koushi-core/src/account/recovery_backup.rs`:
  - extracted the proven edge's admission into
    `admit_secure_backup_for_proven_connectivity` and added
    `replay_secure_backup_admission_on_promotion`, which starts the pending
    inspection and retires its deadline when connectivity was proven before
    promotion, and re-arms the bounded wait when connectivity is still unproven
    at promotion. It replays an inspection rather than the reconnection monitor
    because cancellation resets the recovery epoch on every demotion, so no
    inspection can have settled between the retained edge and promotion.
  - an admitted request that cannot spawn because the session is unpromoted now
    stays pending instead of being dropped silently, so promotion can replay it.
  - `handle_secure_backup_defer_deadline_expired` releases the finished
    deadline handle even when the expiry is not current (it was unpromoted or
    under an older generation). The sender task is finished, and leaving the
    handle behind blocked every later defer from arming a fresh bounded wait,
    which could strand `Checking` with no deadline, terminal, or monitor.
- `crates/koushi-core/src/account/trust_gate.rs`: both session-promotion paths
  (`promote_recovered_session_runtime` and the verified-projection promotion)
  call the replay next to the Secure Backup observer start.
- `docs/architecture/state-machine.md`: records that Secure Backup admission
  does not depend on the ordering of the connection edges, promotion, and
  deferred expiry.

Coalescing and generation fencing are unchanged: the replay clears the pending
flag before delegating, and `start_secure_backup_inspection` still coalesces
onto an in-flight inspection and captures `trust_generation` for its completion
fence.

## Verification

- Three headless actor regression tests, each RED before its production change
  and GREEN after:
  - `proven_connectivity_before_promotion_starts_the_pending_inspection_on_promotion`
  - `proven_connectivity_before_the_request_keeps_it_pending_until_promotion`
  - `deferred_deadline_expiring_before_promotion_is_rearmed_on_promotion`
- `cargo test -p koushi-core --lib`: 1524 passed, 0 failed, 10 ignored.
- `cargo test -p koushi-core --lib account::recovery_backup::tests`: 43 passed,
  including the existing unproven-deadline, coalescing, and
  proven-before-deadline cases.
- `cargo test -p koushi-core --test {runtime_stack,sliding_sync_diagnostics,link_preview,media_save,native_artifact_boundary}`
  green.
- `cargo fmt --check` and
  `cargo clippy -p koushi-core --all-targets --locked -- -D warnings` green.
- Independent read-only review of the integrated diff found the unpromoted
  expiry-handle gap and the dropped unpromoted admission; both were fixed and
  covered by the tests above.

## Pending manual checks

Native two-account startup on a real device (the original report) remains a
manual confirmation; the headless actor test is the correctness evidence.
