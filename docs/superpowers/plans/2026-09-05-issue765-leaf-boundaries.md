# Issue #765 — Leaf crate boundaries and Core edge cleanup

Status: approved and in progress. Fireworks `reviewer-flash` returned
`CORRECT-TO-IMPLEMENT` for exact design head
`b32968a7049823fff4ed5b63377b1d126e74e1f2`; canon PR #788 merged before
implementation.

## Objective

Finish the remaining architecture-umbrella seams as four small, sequential,
independently mergeable implementation PRs. Preserve every command, event,
snapshot, persistence format, credential name, QA token and runtime lifecycle.
This is dependency cleanup, not a product rewrite.

The shortest correct result is deliberately smaller than the issue's historical
inventory: search already delegates pure work to `koushi-search`; media already
delegates image preparation to `koushi-media`; SDK/state mappings encode real
trust, privacy and product semantics; scheduled-send/composer/navigation already
follow owner-named layering; and child actors already use typed mailboxes. Those
areas receive guards or narrowly evidenced moves, not speculative frameworks.

## Live boundary inventory

At base `9f4b4284de8731b0f047ec00933f564660647b77`:

- no `koushi-store` or `koushi-core-testkit` package exists;
- `koushi-core` owns `credential_vault.rs`,
  `store/credential_backend.rs`, and six copies of the same ChaCha20-Poly1305
  12-byte-nonce magic/nonce/ciphertext envelope;
- `StoreActor` is Core's only store actor and owns account path selection,
  unlock-secret acquisition, SDK store/search configuration, generation fences,
  migration order and coarse Core failures;
- `koushi-core` self-depends under `[dev-dependencies]` only to enable
  `test-hooks` for 38 shared-support integration-test targets;
- `koushi-search` already owns document mutation, CJK query variants, candidate
  verification and maintenance; Core owns actor lifecycle, SDK supplement,
  generations, crawler scheduling, diagnostics and actions/events;
- `koushi-media` already owns decode limits, resize/format policy and deterministic
  encoding. The remaining pure duplicate seam is Core's five-format image-byte
  classifier used by the renderable-thumbnail cache;
- no field-for-field SDK/state DTO duplicate survives inspection. Room/profile,
  trust and backup mappings trim/fallback, enforce scope, convert trust enums or
  redact different fields;
- production account/room/timeline modules still import three items from
  `runtime`: `ForwardedComposerDraftPermit`, `ACTOR_MESSAGE_QUEUE_CAPACITY`, and
  `space_member_forward_failure_action`;
- AccountActor already communicates with room/timeline through typed handles and
  message enums, but it still invokes concrete child actor constructors;
- scheduled-send, composer and navigation have distinct owner-local
  model/runtime/store/SDK-IO files matching the existing read-state layering;
  only the crate-root room-key recovery model is separated from its sole timeline
  owner.

## Dependency direction

```text
koushi-state / koushi-protocol / koushi-key / koushi-diagnostics
                         |
                         v
                    koushi-store
  credential backend + encrypted-file envelope + credential vault file
                         |
                         v
                     koushi-core
 StoreActor policy/path/key ownership + actors/runtime/projection/SDK use

koushi-media / koushi-search remain pure algorithm leaves consumed by Core.

koushi-core-testkit --(test-hooks only)--> koushi-core
```

`koushi-store` is a native persistence boundary, not a pure/wasm crate. It may
use filesystem paths and cryptography. It must not depend on Matrix SDK, Tauri,
Tokio, Core, QA or OS keyring implementations. The injected OS credential port
remains `koushi-key::CredentialBackend`; Tauri still supplies its implementation.

`koushi-core-testkit` is a workspace test package, excluded from default
production members. It has no production consumer and is the only package that
enables Core's `test-hooks` for moved integration tests.

## PR 1 — Extract the persistence leaf

### Move

1. Add default production package `koushi-store`.
2. Move the credential-vault file/data implementation and complete credential
   backend (OS-port wrapper, in-memory backend, debug/test file backend and fault
   probes) from Core without changing constants, credential names, file formats,
   cfg gates, diagnostics or error classification. Move backend/vault-internal
   tests with their owner; keep StoreActor-mediated lifecycle/migration tests in
   Core and update imports only.
3. Add one closed encrypted-envelope helper in `koushi-store` for
   `magic || 12-byte nonce || ciphertext`, parameterized by exact magic, 32-byte
   derived key and maximum payload size. Move the existing atomic-replace helper
   to `koushi-store` as the one native persistence primitive and repoint its Core
   settings/store consumers; leave no Core compatibility copy.
4. Replace the six Core-local encrypt/decrypt copies for composer drafts,
   navigation, room preferences, scheduled sends and read-state V1/V2 with that
   helper. Keep each schema version, magic, maximum, JSON shape and migration
   order in its current owner.
5. Keep `StoreActor` in Core. It alone selects paths, loads/creates account
   secrets, derives purpose-specific keys, maps failures to `CoreFailure`, owns
   generation fences and supplies SDK store/search configuration.
6. Propagate Core `test-hooks` to `koushi-store/test-hooks`; expose only the
   existing test-only vault fault/file controls behind that feature. Make
   `koushi-qa` consume the moved debug/test credential-backend probe directly
   under its `qa-bin` feature rather than through a Core compatibility export.
7. Update the macOS QA and Rust structure source guards to inspect the new
   credential owner.

### Do not

- move `StoreActor`, account paths, SDK configuration or actor lifecycle;
- create a generic repository/DAO trait;
- change an encryption key label, magic, nonce size, limit, atomic-write/fsync
  policy, migration sequence or health classification;
- put OS keyring code, Matrix SDK, Tauri or async runtime dependencies in
  `koushi-store`;
- leave a Core compatibility module for the moved credential implementation.

### Verify first

Before moving code, add a structural self-test that fails a synthetic/current
layout when `koushi-store` is absent, when it has forbidden dependencies, when
credential/vault definitions or direct ChaCha imports remain in Core, or when
Core no longer owns `StoreActor`. Preserve the existing encrypted-store tests as
behavioral byte/round-trip/corruption/migration evidence.

## PR 2 — Move shared integration support to a test package

### Move

1. Add non-default, `publish = false` `koushi-core-testkit`.
2. Move `crates/koushi-core/tests/support/mod.rs` and the exact 32 integration
   targets that consume that support or Core `test-hooks` into the testkit
   package. Test bodies and synthetic fixtures remain byte-identical. The target
   manifest is enforced by `check-leaf-crate-boundaries.mjs`.
3. Keep the four self-contained integration targets (`link_preview`,
   `media_save`, `native_artifact_boundary`, `sliding_sync_diagnostics`) in Core;
   they use neither the shared support nor Core `test-hooks`. Also keep Core
   unit-only `account/test_support`, `timeline/test_support`,
   `store/test_support` and single-owner fixtures beside their private owners.
4. Make the testkit depend on Core with `features = ["test-hooks"]`; remove the
   Core self-dev-dependency. Add an explicit CI
   `cargo test -p koushi-core-testkit` step because the package is intentionally
   excluded from default members.
5. Update Rust test-structure/path guards to the authoritative package.

### Do not

- expose private actor constructors merely to make a reusable fixture;
- make the testkit a production/default dependency or QA runtime;
- add fixture builders that have only one consumer;
- rename tests or weaken their assertions.

### Verify first

Tighten the structural self-test so it fails while Core self-depends, shared
support or any of the 32 named targets remains under Core, any of the four local
targets moves unnecessarily, the testkit is default/production, or a target
name disappears.

## PR 3 — Align pure search/media leaves and record intentional mappings

### Move

1. Move Core's `CachedImageKind`/five-format signature classifier and its tests
   to `koushi-media`; update the renderable-thumbnail cache to consume it.
2. Extend the leaf-boundary guard so `koushi-search` and `koushi-media` cannot
   gain Core, protocol, SDK, Tauri, Tokio, filesystem/platform or QA dependencies
   (their existing pure dependencies remain allowed).
3. Record in the worklog that Core search orchestration already consumes
   `SearchDocumentStore`, CJK variants and exact candidate verification, so no
   actor/crawler move is justified.
4. Record the audited SDK/state mapper families and why each retained mapper is
   semantic/privacy-bearing. Delete a mapper only if implementation discovers a
   truly field-for-field copy and a pre-existing contract test proves it.

### Do not

- move SearchActor, crawler lifecycle, SDK queries, generations, diagnostics or
  actions/events into `koushi-search`;
- move media registries, cache lifecycle, diagnostics, state DTO projection or
  platform delivery into `koushi-media`;
- collapse encoded byte variants with serializable upload-selection metadata;
- remove trust/privacy mappings to reduce LOC.

### Verify first

The structural self-test fails while the classifier remains in Core or either
pure leaf accepts a forbidden dependency. Existing media variant/cache/URI and
search edit/redaction/attachment/highlight tests remain the behavior gates.

## PR 4 — Remove real reverse edges and co-locate the remaining model

### Move

1. Move `ForwardedComposerDraftPermit` from `runtime/composer.rs` to the existing
   composer lifecycle owner; update account/timeline imports.
2. Move actor mailbox capacity to the Core crate boundary and move
   `space_member_forward_failure_action` to Core command policy. Production
   child modules must not import `crate::runtime` afterward.
3. Put child construction behind `RoomActorHandle` and
   `TimelineManagerHandle` constructors so AccountActor owns only typed handles
   and messages, not concrete child actor implementation types. Do not add a
   trait or generic actor wrapper.
4. Move the crate-root room-key recovery model under the timeline recovery owner
   and delete the root module. Keep every type, token, backoff and diagnostic
   unchanged.
5. Leave scheduled-send, composer and navigation owner-local layers in place:
   their similarly named state/runtime/store/account/timeline modules are the
   intended read-state-style decomposition, not duplication.

### Verify first

The structural self-test fails on production `crate::runtime` imports under
account/room/timeline, concrete `RoomActor::`/`TimelineManagerActor::` calls from
account, or a crate-root room-key recovery model. Existing lifecycle, shutdown,
command-correlation, scheduled-send, composer, navigation and recovery suites
remain the behavior gates.

## Independent review and merge sequence

Each PR is independently mergeable and runs this gate in order:

1. start from current `origin/main`;
2. add the smallest RED structural/behavior check before the move;
3. implement only that PR's approved slice;
4. run focused tests, full workspace/Tauri/frontend gates as relevant, formatting,
   dependency/security checks and exact diff review;
5. obtain a different-family Fireworks `reviewer-flash` exact-diff verdict;
6. open the PR, require all hosted checks, merge, and rebase the next slice.

The approved design/canon PR lands before PR 1. A finding that changes ownership,
formats, features or package direction stops implementation and requires a design
amendment plus re-review.

## Final acceptance

- `koushi-store` owns credential/vault and encrypted-envelope mechanics without
  SDK/Tauri/runtime/platform-backend leakage; Core `StoreActor` remains fail-closed
  policy owner.
- Core has no self-dev-dependency; shared integration support lives only in the
  non-default testkit.
- pure search/media boundaries are enforced and behavior stays unchanged.
- no production child module imports runtime internals; AccountActor constructs
  children through typed handles/messages; no generic framework exists.
- intentional SDK/state mappings and owner-local feature layers are documented,
  not collapsed.
- all focused/full local gates, exact-diff reviews and hosted CI are green for
  each merged slice.
