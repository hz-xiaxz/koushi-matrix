# HEIC/HEIF Upload Decoder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Select a distributable HEIF decoder, then reuse the bounded image preparation pipeline for HEIC/HEIF still-image conversion and original fallback.

**Architecture:** A mandatory evidence gate selects the dependency. `koushi-media` decodes HEIF into the existing bounded pixel path; core staging, output variants, encryption, retry, and Matrix metadata remain shared.

**Tech Stack:** Rust 2024, `image`, selected HEIF decoder, koushi-media/core tests, Tauri/React staging tests, platform CI packaging.

## Global Constraints

- No optional end-user OS codec dependency.
- Preserve exact original bytes as an explicit fallback.
- Reject unsupported HDR/gain maps unless a tested SDR tone map exists.
- Never log filenames, bytes, EXIF, thumbnails, or Matrix identifiers.
- Do not modify `vendor/matrix-rust-sdk`.

---

### Task 1: Decoder Selection Gate

**Files:**
- Create: `docs/research/2026-08-05-heif-decoder-evaluation.md`
- Modify only after selection: `Cargo.toml`
- Modify only after selection: `Cargo.lock`
- Modify only after selection: `crates/koushi-media/Cargo.toml`

**Interfaces:**
- Produces: one selected decoder/version or an explicit blocked decision.

- [ ] **Step 1: Inventory candidates from primary sources**

For each viable maintained decoder, record license/patent terms, supported
brands/profiles, orientation/color/alpha/HDR behavior, native build inputs,
minimum OS/toolchain requirements, and reproducible CI instructions. Record
Element client behavior and any intentional divergence.

- [ ] **Step 2: Build isolated probes**

Use a temporary crate outside the repository source tree. Decode the same
licensed/generated opaque and rotated fixtures on Linux, macOS, and Windows;
record exact build command, exit status, and artifact-size delta against the
current release build.

- [ ] **Step 3: Apply the pass/fail gate**

Select only a candidate that builds on all three targets, enforces limits, and
has acceptable distribution terms. If none passes, commit only the evaluation
with status `blocked`; do not add a dependency or platform fallback.

- [ ] **Step 4: Pin the selected dependency minimally**

Add one workspace dependency and enable only required features. Run
`cargo tree -p koushi-media` and record native/runtime packaging requirements.

### Task 2: Add Content Detection And Bounded Decode Tests

**Files:**
- Create: `crates/koushi-media/tests/fixtures/heif/README.md`
- Add: licensed/generated fixtures under `crates/koushi-media/tests/fixtures/heif/`
- Modify: `crates/koushi-media/tests/image_variants.rs`
- Modify: `crates/koushi-media/src/lib.rs`

**Interfaces:**
- Produces: internal HEIF brand detection and bounded primary-still decode.
- Consumes: existing `ImagePreparationPolicy` limits and output encoders.

- [ ] **Step 1: Add RED fixture tests**

Cover opaque, rotated, malformed/truncated, oversized, MIME-mismatch,
unsupported sequence/profile, and alpha when supported. Assert orientation-
adjusted dimensions and typed coarse failures.

- [ ] **Step 2: Verify RED**

Run: `cargo test -p koushi-media --test image_variants`

Expected: HEIF cases fail as `ImagePreparationError::Decode`.

- [ ] **Step 3: Implement bounded detection/decode**

Parse only the bounded `ftyp` header needed for brand classification, then call
the selected decoder under existing dimension/allocation limits. Apply
orientation and sRGB conversion before `scale_linearly`; reject unsupported
HDR/gain-map content.

- [ ] **Step 4: Reuse existing encoders**

Feed decoded pixels into existing PNG/JPEG/WebP encoding and recommendation
logic. Add no HEIF output encoder.

- [ ] **Step 5: Verify GREEN and regressions**

Run `cargo test -p koushi-media --test image_variants` and all existing
`koushi-media` tests. Expected: HEIF and unchanged PNG/JPEG/WebP cases pass.

### Task 3: Extend Core Classification And Metadata Contract

**Files:**
- Modify: `crates/koushi-core/src/media_preparation.rs`
- Modify: focused media preparation tests in `crates/koushi-core/src/media_preparation.rs`

**Interfaces:**
- Produces: HEIC/HEIF classification, Original fallback, compatible prepared variants, and metadata derived from selected output.

- [ ] **Step 1: Add RED core tests**

Assert content detection overrides MIME/extension hints; converted extension,
Content-Type, Matrix mimetype, dimensions, byte count, thumbnail, and bytes
agree; Original preserves exact bytes and normalized source MIME. Cover retry
and encrypted/unencrypted parity.

- [ ] **Step 2: Implement minimal classification**

Route supported HEIF content through existing image preparation. On typed
decoder failure retain Original and project one actionable coarse failure; do
not synthesize a second staging machine.

- [ ] **Step 3: Add privacy-safe diagnostics tests**

Assert only brand/backend/outcome/dimension/flag/count fields are formatted and
that filename, EXIF, bytes, paths, room IDs, and event IDs are absent.

- [ ] **Step 4: Run focused Rust and wire gates**

```bash
cargo test -p koushi-core --lib media_preparation
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib frontend_app_state_golden
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml core_event_wire_format_matches_checked_in_contract_artifact
npm --prefix apps/desktop run typecheck
```

The existing Original/ready/failed staging DTO variants cover this behavior;
do not change DTO shapes or golden artifacts.

### Task 4: Wire Existing Staging UI And Final Gates

**Files:**
- Modify: `apps/desktop/src/components/dialogs.tsx`
- Modify: `apps/desktop/src/components/dialogs.test.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/e2e/basic-operations.spec.ts`

**Interfaces:**
- Consumes: Rust-owned Original/prepared variants and coarse failure.
- Produces: compatibility warning and converted-output recommendation rendering.

- [ ] **Step 1: Add browser RED tests**

Assert HEIF enters the existing staging dialog, Original remains selectable,
conversion failure explains fallback, and selected JPEG/WebP/PNG dispatches the
existing output selection without React-derived media semantics.

- [ ] **Step 2: Add only missing presentation copy**

Render the Rust-owned variant/failure data using English/Japanese strings. Do
not decode, sniff, or choose output format in React.

- [ ] **Step 3: Run focused frontend gates**

Run dialog/i18n tests, the focused browser media scenario, typecheck, lint, and
IME inventory if any caption surface changes.

- [ ] **Step 4: Run integrated gates once**

Run current `--server=both --scenario=media`, platform release/package smoke on
Linux/macOS/Windows, secret scan, SDK submodule guard, `cargo fmt --all -- --check`,
and `git diff --check`. Record each command's own exit status and measured
artifact-size deltas.

- [ ] **Step 5: Self-review and commit**

Read the complete diff including new fixtures, confirm fixture licenses and
privacy, verify no HEIF parallel state machine or optional OS dependency was
added, then commit the coherent issue changes.
