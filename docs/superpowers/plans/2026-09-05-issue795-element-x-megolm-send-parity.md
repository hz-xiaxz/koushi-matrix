# Issue #795 — Element X Megolm send parity

Status: shipped in PR #796.

## Objective

Delete every Koushi Megolm send-side repair, manual repair control, scheduler, and SDK fork hook. The resulting production send path must match Element X / stock matrix-rust-sdk structurally: member sync when needed, dirty/untracked device-key query, exactly one `preshare_room_key`, then encryption. Preserve receive-side decrypt retry, backup lookup, key requests, upstream full-member-reload rotation, and Issue #794 read-only persisted rotation attribution.

## Canon and ownership

- `REPOSITORY_RULES.md`, `docs/architecture/overview.md`, and `docs/policies/engineering-rules.md` now make stock Element X send parity normative.
- The user's explicit #795 decision approves removal of the superseded readiness-fence and repair canon.
- SDK changes remain a small topic commit in the vendored fork and are recorded in `docs/upstream/matrix-rust-sdk-feedback.md`.
- Removed product commands/events are deleted through Rust protocol, Core, Tauri, TypeScript, tests, and generated wire artifacts together. React gains no replacement behavior.

## Verify-first baseline

Before deletion:

- the acceptance inventory command found 495 references under `crates/`, `apps/`, and vendored SDK crates;
- the current SDK `ensure_room_encryption_ready` calls `preshare_room_key_with_readiness`, then conditionally enters initial-share repair, proving the send path is not stock;
- existing local QA exercises manual force/share/resend commands rather than the required ordinary-send rotation proof.

The permanent structure guard must reject production/test references to:

```text
force_reshare
share_index0
resend_index0
force_new_outbound
preshare_room_key_with_readiness
with_encryption_sync_readiness
index0_duplicate_share
initial_share_repair
RoomKeyReshare
```

outside historical documentation. A focused SDK test must assert one standard pre-share before encryption and no second share/fence path.

## Implementation

### 1. Remove Koushi product surfaces

- Split `timeline/room_key_recovery.rs`: keep only receive-side decrypt retry, backup lookup, and key-request handling; delete delayed re-share types, scheduler state, account-work kind, diagnostics, manager messages, and tests.
- Delete reshare/force-new/share-index-0/resend-index-0 SDK wrappers and types.
- Delete room command/event/request-outcome variants, encryption-debug handlers, Tauri commands/registrations/builders, frontend client/API methods, Room Info debug buttons/state, mocks, generated wire entries, and manual-command QA evidence.
- Regenerate checked-in protocol artifacts with the repository generator; do not hand-maintain stale wire variants.

### 2. Remove vendored send repair machinery

- Restore `ensure_room_encryption_ready` to member sync → key query → `preshare_room_key` → joined check.
- Delete readiness-fence builder/state/registry/transport-lock code and tests.
- Delete #510/#523 send-future branches, flags, wakes, repair diagnostics that have no remaining observer, and integration tests.
- Delete #538/#541 manual room APIs and crypto helpers, manual `ShareRequestKind`, original-recipient ledger, pickle request-drop migration, and their tests where no receive-side behavior uses them.
- Keep rotation classification/attribution (#794), initial standard-share diagnostics that instrument the stock path, member-reload rotation, backup upload, key request/gossip, and receive diagnostics.

### 3. Replace QA with ordinary-send parity proof

Add/modify a local two-client headless scenario that:

1. establishes an encrypted room and confirms ordinary decryption;
2. forces a normal rotation using membership change or message-count expiry, not a deleted API;
3. sends through the ordinary timeline path;
4. proves the receiver decrypts and its inbound session has `first_known_index == 0`;
5. repeats with sender runtime restart between the rotation trigger and ordinary send;
6. emits private-data-free fixed evidence tokens only.

Run against local Tuwunel and Synapse where the existing core lane supports both. Element Web/X interoperability is a final spot-check only after deterministic local proof.

## Verification gates

Focused while iterating:

```bash
rg -n 'force_reshare|share_index0|resend_index0|force_new_outbound|preshare_room_key_with_readiness|with_encryption_sync_readiness|index0_duplicate_share|initial_share_repair|RoomKeyReshare' crates apps vendor/matrix-rust-sdk/crates
(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk-crypto)
(cd vendor/matrix-rust-sdk && cargo test -p matrix-sdk)
cargo test -p koushi-sdk --lib
cargo test -p koushi-core --lib
cargo test -p koushi-protocol
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test
```

Before PR, run the full repository gate matrix from `docs/agents/verification.md`, the new local two-client scenario on supported servers, both format checks, SDK submodule guard, diagnostics/privacy guards, agent-doc checker, secret scan, and `git diff --check`.

## Acceptance mapping

- Zero forbidden references: closed grep plus permanent structure check.
- Exactly one share step: focused SDK send-path test and finished diff against stock upstream.
- Rotation and sender-restart decryption at index 0: local two-client QA evidence.
- Interop: manual Element Web and Element X spot-check after local automation is green.
- #794 coexistence: persisted exact reason tests remain green and attribution code does not enter send control flow.
