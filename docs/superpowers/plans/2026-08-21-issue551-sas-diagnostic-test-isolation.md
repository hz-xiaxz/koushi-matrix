# Issue #551 SAS diagnostic test isolation

Status: design review pending. Independent blocker repair for TimelineView subscription PR #606.

## Failure evidence

`account::verification::tests::actor_sas_settlement_emits_exactly_one_terminal_and_clears_runtime` failed repeatedly under full-suite concurrency while passing focused 3/3:

- local full suite observed extra flow 83 before/after flows 100–104;
- PR #607 initial CI observed `[100,101,102,103,104,83]`;
- PR #606 rerun observed `[100,101,83,102,103,104]`.

The test holds `koushi_diagnostics::test_support::lock()` and slices the global ring from its starting length, but the lock only serializes cooperating test bodies. An asynchronous diagnostic producer admitted by an earlier test can complete after this test's snapshot and append its own `core.sas_verification/settled` record. The varying position of flow 83 proves cross-test tail completion, not a duplicate terminal from this actor.

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
- Global diagnostics privacy, capacity and lock semantics remain unchanged.

## Review gate

- Design pending `reviewer-flash` read-only verdict.
- Implementation prohibited until `Correct-to-implement`.
- Full diff and delivery pending.
