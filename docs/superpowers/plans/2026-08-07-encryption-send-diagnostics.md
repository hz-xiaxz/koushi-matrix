# Encrypted Send Diagnostics Implementation Plan

> **For Codex:** Execute this plan task-by-task with focused tests before broader verification.

**Goal:** Add privacy-safe diagnostics that distinguish sender-side recipient/session problems from receiver-side delayed decryption when another Matrix client cannot decrypt a Koushi message.

**Architecture:** Poll a read-only local-store encryption snapshot concurrently inside the manager-owned Matrix SDK enqueue worker, add a correlated post-terminal session snapshot in a capacity-bounded manager task set, and enrich the existing bounded room-key reshare diagnostics with attempt and target information. Reuse existing Matrix SDK state and do not change encryption, device tracking, retry timing, or send behavior.

**Tech Stack:** Rust, matrix-rust-sdk, koushi-diagnostics, Tokio tests.

---

### Task 1: Define and test privacy-safe diagnostic projection

**Files:**
- Modify: `crates/koushi-core/src/timeline.rs`

- [x] Add failing tests for encrypted-send snapshot fields and privacy constraints.
- [x] Add failing tests for room-key reshare attempt/target fields.
- [x] Run the focused tests and confirm the expected failures.

### Task 2: Capture encrypted-send and reshare diagnostics

**Files:**
- Modify: `crates/koushi-core/src/timeline.rs`

- [x] Collect room encryption state, recipient strategy, outbound-session presence, own-user tracking status, and aggregate own-device counts without blocking enqueue.
- [x] Record the snapshot under the existing send correlation without identifiers or key material.
- [x] Record each scheduled/executed reshare attempt with target, delay, outcome, request count, and recipient count.
- [x] Keep all send, key sharing, and retry behavior unchanged.

### Task 3: Verify and publish

**Files:**
- Modify: `docs/superpowers/plans/2026-08-07-encryption-send-diagnostics.md`

- [x] Run focused tests, formatting, and a package check.
- [x] Review the final diff for privacy and scope.
- [x] Commit, push `codex/encryption-send-diagnostics`, and open a draft pull request.
