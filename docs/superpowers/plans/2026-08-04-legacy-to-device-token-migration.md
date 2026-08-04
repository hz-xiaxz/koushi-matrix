# Legacy To-Device Token Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make stores last used by classic `/sync` start Koushi's mandatory Simplified Sliding Sync without deleting sessions, crypto keys, or room caches.

**Architecture:** Classify the crypto store's shared to-device token at the Matrix Rust SDK Sliding Sync restore boundary. Retain only non-empty ASCII-decimal Sliding Sync tokens; omit legacy compound tokens so the first successful response replaces them through the existing store path. Build the DMG immediately after the minimal red-green fix, then add diagnostic projection and broader regression coverage before the PR.

**Tech Stack:** Rust, Matrix Rust SDK submodule, Koushi diagnostics, Cargo tests, Tauri macOS DMG tooling.

## Global Constraints

- Koushi production remains Simplified Sliding Sync-only.
- Never delete or recreate the account, crypto, state, event-cache, or search stores.
- Never log or serialize the token value, length, prefix, homeserver URL, or Matrix identifiers.
- Preserve valid non-empty ASCII-decimal tokens byte-for-byte.
- Build the local arm64 DMG before broader test and PR work.

---

### Task 1: Token Compatibility Migration

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/sliding_sync/cache.rs`

**Interfaces:**
- Consumes: `OlmMachine::store().next_batch_token() -> Result<Option<String>>`
- Produces: `classify_to_device_token(Option<String>) -> (Option<String>, ToDeviceTokenFormat)` used only by Sliding Sync restoration.

- [ ] **Step 1: Write the failing classification test**

Add an inline unit test asserting `None` stays absent, `"42"` is retained as Sliding, and `"s123_4_5"`, `""`, and non-ASCII digits are omitted as Legacy.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p matrix-sdk sliding_sync_to_device_token_classification --features e2e-encryption`

Expected: FAIL because `classify_to_device_token` and `ToDeviceTokenFormat` do not exist.

- [ ] **Step 3: Implement the minimal classifier and restore integration**

Define a private fixed enum with `Absent`, `Sliding`, and `Legacy`, classify with `!token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit())`, and assign only the retained token to `RestoredFields::to_device_token`.

- [ ] **Step 4: Run the focused test**

Run: `cargo test -p matrix-sdk sliding_sync_to_device_token_classification --features e2e-encryption`

Expected: PASS.

### Task 2: Fast Local DMG

**Files:**
- Build output: `target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`
- Copy output: `/Users/hiroshi/projects/Element-dev/matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

**Interfaces:**
- Consumes: the Task 1 SDK patch and existing diagnostic changes.
- Produces: an installable local DMG for the affected account database.

- [ ] **Step 1: Format the touched SDK file**

Run: `cargo fmt --manifest-path vendor/matrix-rust-sdk/Cargo.toml --all -- --check`

- [ ] **Step 2: Build without the full preflight suite**

Run: `npm --prefix apps/desktop run build:dmg -- --skip-preflight`

Expected: exit 0 and a new arm64 DMG under `target/release/bundle/dmg`.

- [ ] **Step 3: Copy and verify the artifact**

Run `hdiutil verify` and `shasum -a 256`, then copy the artifact to the canonical main-workspace DMG path.

### Task 3: Formal Diagnostics And Regression Coverage

**Files:**
- Modify: `vendor/matrix-rust-sdk/crates/matrix-sdk/src/sliding_sync/cache.rs`
- Modify: `crates/koushi-core/src/sliding_sync_diagnostics.rs`
- Modify: `crates/koushi-core/tests/sliding_sync_diagnostics.rs`
- Modify diagnostics transport files already changed on this branch as required by the snapshot schema.

**Interfaces:**
- Produces only coarse fields: `to_device_token_format=absent|sliding|legacy` and `legacy_to_device_token_migration_applied=true|false`.

- [ ] **Step 1: Add failing request-level and privacy tests**

Prove a legacy token is absent from a generated Sliding Sync request, a valid decimal token is retained, and serialized diagnostics cannot carry raw token material.

- [ ] **Step 2: Run focused tests and observe the expected failures**

Run the exact Matrix SDK and `koushi-core` test targets containing the new cases.

- [ ] **Step 3: Add the minimal fixed-enum diagnostic plumbing**

Expose no free-form string at the public diagnostic boundary and record migration before the first Sliding Sync request.

- [ ] **Step 4: Run focused tests until green**

Expected: all new and adjacent tests pass with no failures.

### Task 4: Verification, Commit, And PR

**Files:**
- Modify: `docs/upstream/matrix-rust-sdk-feedback.md` if the pinned SDK patch requires provenance documentation.

**Interfaces:**
- Produces: one reviewed branch and a ready-for-review GitHub PR.

- [ ] **Step 1: Run formatting and targeted checks**

Run Rust formatting, the affected Matrix SDK tests, affected Koushi core tests, diagnostic frontend tests, and `cargo check` for the touched production crates.

- [ ] **Step 2: Review the complete diff and secret scan**

Confirm the diff contains no raw token logging, unrelated generated files, or accidental user changes.

- [ ] **Step 3: Commit intentionally**

Commit the SDK migration, diagnostics, tests, docs, and existing Sliding Sync runtime diagnostics as logically scoped commits.

- [ ] **Step 4: Push and open a ready-for-review PR**

Push `codex/sliding-sync-runtime-diagnostics`, create the PR against `origin/main`, and report the URL and verified DMG checksum.
