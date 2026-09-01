# Issue #765 leaf-boundary worklog

## Design and canon gate

- Base after #763: `9f4b4284de8731b0f047ec00933f564660647b77`.
- Design/canon: `docs/superpowers/plans/2026-09-05-issue765-leaf-boundaries.md`.
- Initial design commit: `4bf82e50a7ac69b0dcf5c1c24121994eaea3c3ef`.
- Fireworks `reviewer-flash` found four minor gaps: test ownership partition,
  atomic-file helper ownership, direct QA/store feature wiring, and incorrect
  XChaCha naming.
- Fixed design head: `b32968a7049823fff4ed5b63377b1d126e74e1f2`.
- Fireworks re-review verdict: `CORRECT-TO-IMPLEMENT`.
- Canon PR #788 passed all eight hosted jobs and merged as
  `cfa56c1ea55ed5be80b643c8e3809596b872dc3a`.

## Inventory decisions

- Extract a real persistence leaf: credential/vault implementation, one exact
  ChaCha20-Poly1305 envelope, and the native atomic replace primitive.
  `StoreActor` remains Core's fail-closed policy/lifecycle owner.
- Move shared Core integration support/targets to a non-default testkit; keep
  owner-private unit fixtures local.
- Move only the pure image byte classifier to `koushi-media`. Search already
  delegates all pure work to `koushi-search`; no actor/crawler move is justified.
- Retain SDK/state mappings that normalize identity, trust, scope or redaction.
- Remove only real runtime reverse edges and concrete child-constructor calls;
  do not add traits or a generic actor framework.
- Scheduled-send, composer and navigation already use deliberate owner-local
  layers. Only the root room-key model needs physical co-location.

## PR1 verify-first evidence

Commit `9aaecda344b9439762f836b2ef47b1e684581066` added the persistence-leaf
checker before implementation.

- checker self-tests: 3 passed;
- baseline integration-test inventory for PR2: 36 targets plus one shared
  support module, with path/content hashes captured before moves;
- baseline reverse-edge inventory for PR4: 10 production `crate::runtime`
  references and 4 concrete account-owned child-constructor calls;
- current-tree checker: RED, reporting the absent `koushi-store`, retained Core
  credential/vault/ChaCha/atomic implementations, missing feature propagation,
  and stale QA probe route.

Implementation must turn this same checker green without weakening it.

### Persistence exactness ledger

The PR1 diff review must preserve:

- composer drafts: `KOUSHI-DRAFTS-V1\0`, 12-byte nonce,
  `derive_composer_drafts_key`, schema 3 and the existing fault-aware atomic
  replacement;
- navigation: `KOUSHI-NAVIGATION-V1\0`, 12-byte nonce,
  `derive_navigation_key`, and its intentionally distinct writer ordering;
- room preferences: `KOUSHI-ROOM-PREFERENCES-V1\0`, 12-byte nonce and
  `derive_room_preferences_key`;
- scheduled sends: `KOUSHI-SCHEDULED-SENDS-V1\0`, 12-byte nonce and
  `derive_scheduled_sends_key`;
- read-state: V1/V2 magics and versions 1/2, 256 KiB bound,
  `derive_read_state_outbox_key`, conservative V1 migration, generation fence
  and atomic V2-before-V1-removal ordering;
- credential vault: `KOUSHI-CREDENTIAL-VAULT-V1\0`, versions 1/2, 12-byte
  nonce, 16 MiB bound, vault master key, fault injection and no-corrupt-file
  overwrite behavior.

### PR1 integration corrections

The delegated Luna implementation timed out with a failing partial tree. Parent
integration retained the useful move, then corrected the exactness failures
before broad verification:

- fixed an unbounded-envelope `usize::MAX` arithmetic overflow and added bounded
  plus unbounded helper tests;
- restored navigation's deliberately distinct `atomic_replace` writer;
- restored room-preference and scheduled-send direct-write behavior instead of
  silently upgrading them to atomic replacement;
- preserved the original credential-test name/body set, partitioning only the
  three backend-private tests into `koushi-store`;
- removed the dead Core credential implementation and Core compatibility
  re-export;
- gated vault data/file re-exports to tests/test-hooks and redacted
  `CredentialVaultFile` Debug so native paths cannot enter test logs;
- wired the leaf checker into CI and desktop dependency lint.

Focused post-integration evidence:

- leaf checker/self-tests and build-structure contracts: green;
- Rust source-structure checker/self-tests: green;
- `koushi-store`: 13 tests;
- Core store unit slice: 50 tests; full Core lib: 912 passed / 8 ignored;
- login-store lifecycle, local-store migration and pending-login journal: 13
  integration tests;
- both QA binaries: 88 and 13 tests;
- macOS QA source-contract Vitest: 47 tests;
- release checks for `koushi-store` and `koushi-core`: green with no new store
  warning.

Fireworks exact-diff review at `bececb93fa8ad68f982a59e0b78e04a08071c12a`
returned `CORRECT-TO-MERGE` with one cosmetic finding: the macOS QA test title
and comment still named Core as the credential mechanism owner. The wording was
corrected to `koushi-store`; Fireworks re-reviewed exact head
`c1eeda78e53e76a74e00bf64171ad87741c5225a` and returned
`CORRECT-TO-MERGE`.

PR #789 passed all eight hosted jobs and merged as
`d3982a84d1b98c9bb73cc2b22f9a0465233c8892`.

The broad local `qa:headless-basic:local` attempt passed Tuwunel login and
directory, then the unrelated timeline-reconnect oracle rejected a pagination
terminal arriving before `Paginating`. Exact base `cfa56c1e` reproduced the same
failure. The affected credential/login/restore scenarios passed for Tuwunel and
Synapse; #738 remains the separate settlement issue.

## PR2 verify-first evidence

Commit `97293b54dbbc3b44559724c706ca1c61fec8fba0` extended the leaf
checker before the test move.

- checker self-tests: 4 passed;
- current-tree checker: RED for the absent non-default testkit, Core
  self-dev-dependency, shared support/32 hook-consuming targets still in Core,
  and missing explicit CI testkit gate;
- four self-contained Core integration targets are intentionally retained:
  `link_preview`, `media_save`, `native_artifact_boundary`, and
  `sliding_sync_diagnostics`.

The exact 32/4 partition was added to the design and Fireworks returned
`CORRECT-TO-IMPLEMENT` at `45de4b8decc2508a0a9efb250d95265b9e381c60`
before the stashed partial implementation resumed. Two non-blocking checker
coverage findings were fixed by enforcing exact directory target sets and
mutating both duplicate/misplaced branches.

Removing the self-dev-dependency exposed why it had existed: Core unit-test
items used feature-only `test-hooks` cfgs. These were changed mechanically to
`cfg(any(test, feature = "test-hooks"))`, with the three inverse branches changed
to the exact negation. Normal/release production cfg remains byte-equivalent;
bare `cargo test -p koushi-core` now compiles the same hooks under `cfg(test)`
without feature unification. A leaf guard rejects future feature-only unit-test
hooks.

Focused PR2 evidence:

- exact move verifier: support + 32 moved target bodies byte-identical to
  `d3982a84`; four retained Core targets byte-identical;
- testkit: 32 targets / 228 tests;
- Core: 912 passed / 8 ignored plus 22 tests across the four retained targets;
- release Core check, leaf checker/self-tests, Rust structure/self-tests and
  cargo-machete: green.

Fireworks exact-diff review at
`5a2a3778ab199871170dcc70f94ccfeb1245fcaa` returned
`CORRECT-TO-MERGE` with one actionable guard-hardening Minor and one deliberate
plan-mandated CI duplication note. The checker was strengthened to reject any
normal `[dependencies]` table in the testkit; the explicit CI test remains
because the approved plan requires it. Exact-delta re-review remains required.
