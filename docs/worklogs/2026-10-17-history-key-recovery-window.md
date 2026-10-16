# Historical room-key recovery window

## Approved scope

On 2026-10-17 the user chose a 30-day default window after accepting a room
invitation, replacing the SDK's 24-hour default. This patch changes invitation
bundle acceptance and startup recovery through their common SDK predicate.

The follow-up recovery UI agreed in the same conversation starts from an
undecryptable message, uses the Japanese action label
「過去のメッセージの復号鍵を要求」, and requests all shareable historical
keys for that room. The subsequent implementation below adds that distinct
protocol and UI, including Space/public joins without an individual invitation.
Manual requests use all eligible members for a complete joined population of
at most 30, otherwise administrators; startup retries always use administrators.
The existing standard single-event request remains a separate secondary action.

## Contracts and upstream comparison

Consulted `REPOSITORY_RULES.md` (Key Management, Review And Audit),
`docs/architecture/overview.md` (Security Model),
`docs/agents/verification.md`, and
`docs/policies/engineering-rules.md` (Build, Dependencies, QA Gates).
The Security Model and user help document the approved window before the
production duration changes. The SDK feedback packet records the local patch
and upstream configuration intent.

The checked-out Matrix Rust SDK implements both live receipt and startup
cleanup with a 24-hour predicate. Element X iOS enables the same SDK invitation
sharing flow in its client builder. MSC4268 explicitly leaves the definition
of recently accepting an invite to clients and documents the 24-hour
Element Web / Rust SDK choice. Koushi intentionally extends it to 30 days to
allow delayed restart/import; it does not change key-sharing eligibility.

## Review and verification

Independent read-only design review: no blockers, conditional on retaining
inviter, room membership and device-trust checks, rejecting future timestamps,
and retaining expiry cleanup. The new private clock parameter makes exact
boundary tests deterministic. Extending the window does not restore previously
deleted pending entries or regenerate unavailable bundles.

Initial phase files: SDK shared-history predicate and focused tests, Security Model,
room/space help, upstream feedback packet, and this record.

- RED: fixed-clock window tests against the old cutoff: 4 passed, 3 failed
  (24 hours, 29 days, and 30 days minus 1 millisecond), Cargo exit 101.
- GREEN: `cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p
  matrix-sdk --lib room::shared_room_history --locked --offline`: 8 passed,
  including all seven window tests and existing inviter acceptance coverage.
- `rustfmt --edition 2024 --check` on the two SDK source/test files passed.
- `node scripts/user-help.mjs --check` passed for navigation, links, anchors
  and generated `llms.txt`.
- Root and SDK `git diff --check` passed.
- Final independent read-only integrated diff review: no blockers; the main
  agent also inspected the complete scoped diff and new files.
- `cargo test --manifest-path vendor/matrix-rust-sdk/Cargo.toml -p matrix-sdk
  --features testing --test integration
  encryption::shared_history::test_shared_history --locked --offline`:
  2 passed (out-of-order delivery and crash-before-import recovery).
- Both Cargo runs exited 0. They emitted existing warnings in unrelated SDK
  test sources; those files were not changed by this task.

## Real Synapse follow-up

The user required a real Synapse scenario in addition to mock-server tests.
`history_key_recovery_window` uses production Koushi SDK persistent sessions
and the SDK Simplified Sliding Sync service, two rooms, and two independently
rotated historical sessions per room. A temporary public media-fetcher failure
defers import; only existing pending-record timestamps are backdated. A
same-device restart must decrypt all 29-day history and expire 30-day history.
The full oracle, cleanup, and token contracts are in
`docs/qa/history-key-recovery-window.md` and `docs/agents/qa-lanes.md`; the
runner enforces every token and the nightly focused lane registers the scenario.

Independent design review found no blocker for this QA-only approach.
Integrated review required retaining session/sync cleanup owners across restart
and bounding cleanup phases; both were corrected before the live run.
No production bypass or clock override was added.

The original checkout's SDK HEAD differs from its parent gitlink and contains
unrelated work. To preserve it, the live scenario is validated in an isolated
checkout at the parent's exact recorded SDK gitlink, with only this task's
changes copied over. The normal submodule guard passes there. The temporary
checkout reuses the original shared Cargo target directory, which is not removed
at cleanup.

Results against disposable Synapse v1.157.0:

- GREEN: the complete scenario exited 0 with every required token. Both
  historical sessions were distinct; neither room had imported keys before
  restart. The restored device decrypted both 29-day messages, removed both
  pending records, and retained no historical sessions for the expired room.
- RED: temporarily restoring only the old 24-hour cutoff in the isolated SDK
  made that same real-Synapse scenario exit 1 specifically with
  `history window: 29-day import timed out`. The runner removed its disposable
  server after failure. A `finally` block restored the 30-day source.
- Final GREEN after restoring 30 days: exit 0 with all five required tokens.
- QA binary unit tests: 125 passed, including focused-scenario routing/tokens.
- Scoped QA binary clippy with `--locked --offline -- -D warnings`: passed.
- Synapse fixture tests: 6 passed. QA lane/output artifact tests: 11 passed.
- QA lane registration, agents-doc links, protocol/QA boundaries, Rust test
  structure, scoped formatting, and whitespace checks passed.
- Read-only integrated security review after cleanup corrections: no blockers.
- Verified that all scoped task sources match the isolated tested copy, then
  removed both temporary Git worktrees. The shared build directory was kept.

Private-data-checked run evidence is kept in the ignored
`artifacts/history-key-recovery-window/` directory (`synapse-green.log`,
`synapse-red.log`, and the recorded `synapse-red-oracle.log`). No generated
credentials, client databases, or server data are copied there.

Broader pre-existing helper-test limitations: the combined 26-test Node run
had 24 passes and two unrelated failures. The Tuwunel fixture test's expected
object omits the existing `multiReaderReceipts` capability. The retired-scenario
test in the original checkout encounters its pre-existing stale SDK gitlink
guard before reaching the flag guard. Those files/guards were not changed to
mask either result. The scoped Synapse fixture and lane/artifact checks above
pass independently.

## Explicit recovery and startup retry implementation

The later phase uses `codex/history-key-recovery`, based on current `origin/main`,
with the SDK initialized at its recorded gitlink. The original checkout and its
unrelated notification work remain untouched. This phase supersedes the earlier
five-token QA contract with the full contract in the linked scenario document.

Core now owns the account-session recovery actor, authoritative recipient
selection, authenticated Olm request/reply protocol, fresh membership and trust
checks, per-device correlation and encrypted durable state. SDK additions are
limited to generic encrypted custom data, bounded historical export and verified
forwarded import. Tauri and React dispatch/render typed contracts. Failed-message
and collapsed Room Info entry points, Requested/Retry, catalog translations,
account scoping and generated DTO/wire artifacts are included. User help was
checked against actual labels and prerequisites; no Settings property moved.

The root Key Management exception and Security Model were amended before this
protocol implementation. Independent read-only design review approved the
receive-side exception subject to current membership/history eligibility,
mutually verified cross-signing, immutable identity pins, bounded storage and
crash-safe replay. Exact per-device nonce and cached-response designs received
further independent review before their implementation.

Final integrated review identified and corrected three blocking issues:

- Export initially loaded the entire room inventory before enforcing its count
  cap. SQLite now applies LIMIT before decoding and MemoryStore checks under
  its lock; unsupported stores fail closed.
- An operation timeout could cancel an in-flight durable write and continue.
  Every write now has a cancellation poison guard; either operation timeout
  stops the worker, and accepted queued commands settle with their request IDs.
- Identity refresh could outlive the room facts used for disclosure. Final
  server facts now follow identity refresh and quota persistence, with a local
  own-membership/history invalidation check immediately before send. The live
  fixture pauses there, removes the requester, and requires no disclosure.

Self-review also corrected lost completion receipts on target narrowing,
coalescing after a failed request, same-generation Sending regression, missing
closed-mailbox failure settlement and room-v12 additional-creator ordering.
Independent final reread found no remaining security blockers, conditional on
the updated tests and local gates. The reviewer did not execute those gates;
the main agent owns verification and acceptance.

Evidence already obtained before the final security corrections:

- RED/GREEN authoritative state projection and SDK 24-hour boundary cases.
- RED UI stub: both new component tests failed; real component passed.
- Production Core Synapse scenario passed all original extended tokens, including
  offline pending, admin-only startup retry, persisted restart cooldown,
  join without invitation, 31-member admin default, force-all, recovery after
  invitation expiry, visibility/left/blocked guards, and cache crash replay/Ack.
- Chromium: all 4 failed-message / advanced Room Info browser tests passed.
- All 2086 frontend unit tests passed after the exhaustive API inventory was
  updated; focused state-order regression failed before the fix and passed after.

Final post-review gate results and PR identities are recorded below when complete.

Final post-review results:

- Disposable Synapse v1.157.0: complete scenario exit 0, every required token,
  including `history_recovery_policy_race_guard=ok`. The earlier final run
  correctly refused the race but exposed an overbroad global-zero cache oracle:
  the unrelated departed room may retain an unacknowledged bounded response.
  The corrected crash oracle requires the exact original crash-room response
  ID, Tombstone and zero bytes plus successful replay decryption. Independent
  read-only review approved that correction; the complete rerun passed.
- Rust libraries: Core 1669 passed (10 ignored), desktop 257 passed (1 ignored),
  SDK adapter 278 passed (1 ignored), state 100 passed. History-state integration
  4 passed. Core includes exact identity/nonce/expiry/correlation rejection and
  interrupted-write poisoning tests. Regenerated desktop snapshot and Core-event
  golden artifacts passed again without update flags in the full library run.
- QA binary: 125 passed. Core and QA all-targets clippy with warnings denied:
  passed. Core's initial three style warnings and QA's nonminimal-bool warning
  were corrected, without suppressions.
- SDK crypto: 3 focused tests passed. Encrypted SQLite inventory: 1 passed with
  the explicit `crypto-store` feature. The earlier default-feature SQLite run
  selected zero tests and was not counted as evidence; an initial test-macro
  name requirement was fixed before the real test passed.
- Frontend: 2086 passed; typecheck and lint passed. Final Chromium rerun: all
  4 request/Retry/advanced-control tests passed.
- Synapse fixture: 6 passed. QA token/lane/output/protocol helper tests: 15
  passed. SDK gitlink, retained reachability, command snapshots, thin Tauri,
  protocol/QA boundaries, Rust test placement, QA lane coverage, help generation,
  workspace rustfmt and whitespace guards passed.
- Full unrelated homeserver-helper suite retains the known Tuwunel fixture
  expectation failure (missing the existing multiReaderReceipts capability).
  The focused Synapse suite passes. `cargo fmt --all --check` also visits the
  vendored upstream baseline and reports unrelated existing formatting; the
  normative CI command `cargo fmt --check` passes, and changed SDK code was
  formatted separately with its nightly settings. No unrelated expectations or
  upstream files were changed to hide either baseline result.

The SDK commit is `6d40017c8`; the parent records this exact pushed gitlink.
Residual risk remains irreversible sharing to an already verified current room
member and bounded resource use by a malicious verified member. The UI never
claims all messages recovered, and Element X does not answer the custom protocol.
No automatic all-member broadcast or outbound-session repair was added.

An independent final canon clarification approved replacing the overly broad
claim that current outbound keys are never reshared: SDK historical inbound
export can include the inbound copy of an active sending session. An exported
Megolm key has no upper index/time cutoff and can decrypt later indices in the
same session, but need not decrypt earlier indices. The implementation leaves
normal outbound pre-share/rotation unchanged; the security assessment now states
this limitation explicitly. No implementation scope changed.
