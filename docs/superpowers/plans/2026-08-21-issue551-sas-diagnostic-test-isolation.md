# Issue #551 SAS diagnostic test isolation

Status: design review pending. Independent blocker repair for TimelineView subscription PR #606.

## Failure evidence

`account::verification::tests::actor_sas_settlement_emits_exactly_one_terminal_and_clears_runtime` failed repeatedly under full-suite concurrency while passing focused 3/3:

- local full suite observed extra flow 83 before/after flows 100–104;
- PR #607 initial CI observed `[100,101,102,103,104,83]`;
- PR #606 rerun observed `[100,101,83,102,103,104]`.

The test holds `koushi_diagnostics::test_support::lock()` and slices the global ring from its starting length, but the lock only serializes cooperating test bodies. Sibling test `own_user_sas_proof_success_enters_shared_authoritative_promotion_path` uses flow 83 without taking that lock and can append its own `core.sas_verification/settled` record concurrently. The varying position of flow 83 is therefore cross-test completion, not a duplicate terminal from this actor.

## Minimal fix

Change only the test's final diagnostic iterator in `crates/koushi-core/src/account/verification.rs`:

- retain the existing source/stage and `flow_id` parsing;
- after parsing a count, retain only flow IDs `100..=104`, the exact five synthetic flows configured by this test;
- keep the final ordered vector assertion exactly `[100,101,102,103,104]`.

This preserves detection of missing, reordered or duplicate diagnostics for every flow owned by the test while excluding records it cannot attribute to its actor. Do not clear/reset the global ring, weaken the final assertion, add sleeps, change production diagnostics, or alter actor behavior.

## Verification

- RED evidence is the repeated full-suite/CI failure above; focused currently passes because no foreign tail arrives.
- Focused test 5 consecutive runs green after the filter.
- `cargo test -p koushi-core --lib` must pass repeatedly under normal parallel execution.
- `cargo test --workspace --all-targets`, rustfmt, diff check and CI 7/7.
- No frontend/local homeserver gates are behaviorally affected, but the final PR uses the normal full repository gate matrix.

## Scope and invariants

- One test-body expression only; no production item, helper, public API, dependency or fixture change.
- Existing actor terminal action/runtime-clear/stale-duplicate checks remain exact.
- The owned range remains reserved for this test; a future foreign settled diagnostic in 100..=104 must fail loudly rather than be hidden.
- Global diagnostics privacy, capacity and lock semantics remain unchanged.

## Review gate

- Design: `reviewer-flash` traced flow 83 to the unlocked sibling test, verified the exact insertion type and preservation of missing/order/duplicate detection, and recorded `Correct-to-implement`.
- Implementation approved, not started.
- Full diff and delivery pending.
