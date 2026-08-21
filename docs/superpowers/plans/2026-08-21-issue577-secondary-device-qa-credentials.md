# Issue #577 follow-up — isolate same-user QA device credentials

Status: implementation and local disposable-server verification complete.
`reviewer-flash-opencode-go` returned `Correct-to-merge` on 2026-08-21; PR
pending. Its two optional observations (retained ignored per-run credential
artifacts and a positive-list source contract) are non-blocking because each
runner root is disposable and all current same-user fixtures are enumerated.
CI round 1 then caught that the public runtime constructor was `qa-bin`-visible
while its crate-private `StoreActor::with_backend` dependency remained
test-hooks-only. The dependency now has the same narrow `qa-bin` gate without
its test-only composer probe; the non-test QA binary compiles, and focused
read-only re-review again returned `Correct-to-merge`.

## Problem

The disposable `e2ee_trust` core lane timed out while waiting for the A2
`ExistingDeviceSas` gate on both Tuwunel and Synapse. The same lane failed on an
isolated `origin/main` baseline, so this was not caused by #612's encryption
readiness fence.

Privacy-safe instrumentation showed that A2 discovered the existing identity and
recovery methods but no owner-signed proof device. Local store-path comparison
then established the cause: A and A2 opened the same device-scoped SDK account
path. Every `CoreRuntime::start_with_data_dir` used the process-wide QA file
credential store, so a same-user secondary runtime restored the primary saved
session instead of creating another server device. Synapse corroborated this
with repeated one-time-key replacement conflicts for the reused device.

## Design

- Keep production credential and session behavior unchanged.
- Keep restart fixtures on the shared QA credential store because they must
  restore the same device.
- Give every fixture that represents a distinct second device of the same user
  a deterministic credential subdirectory under the runner-owned ignored
  `KOUSHI_QA_FILE_CREDENTIAL_STORE_DIR`.
- Continue giving each fixture its existing independent SDK data directory.
- Expose the existing file-credential constructor to the explicit `qa-bin`
  feature only; do not enable the broader core `test-hooks` feature.
- Add a source contract covering all same-user secondary-device fixture labels
  so a future direct `start_with_data_dir` regression fails fast.
- Export no identifiers or credentials. Verification compares account paths only
  for equality and keeps homeserver artifacts under `.local-secrets`.

## Verify-first evidence

Before the fix:

- the new focused contract failed first at `gate-negative-a2`;
- Tuwunel and Synapse both reached `e2ee_key_backup_enable=ok` and then timed
  out at `session A2 gate`;
- A and A2 account paths were equal.

After the fix:

- the focused contract and all 130 Headless Core QA binary tests pass;
- A and A2 account paths are distinct without printing either value;
- full SDK + core `e2ee_trust` passes on Tuwunel and Synapse, including
  `gate_own_sas`, second-device decryption, unverified/blocked-device policy,
  and multi-user multi-device decryption tokens.

## Final local evidence

- Root workspace: 2,408 passed / 13 ignored.
- Headless Core QA binary: non-test `cargo check` passed; 130 tests passed.
- Tauri: 149 passed / 1 ignored.
- Frontend: typecheck and lint passed; Vitest 1,370 passed; production build
  passed; Playwright 248 passed.
- `cargo deny check`, rustfmt, diff, docs, IME, privacy, and SDK-submodule checks
  passed.
- Full SDK + core `e2ee_trust`: Tuwunel passed and Synapse passed.

Open a PR, require all CI checks green, and merge before recording this #577
validation blocker as cleared.
