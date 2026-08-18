# Issue #547 — memory bounds and preview URL lifetime

Status: implemented and verified locally (2026-08-18)

## Scope and ownership

This change bounds memory retained by image preparation and renderable-thumbnail
caches without changing Matrix or timeline semantics.

- Rust remains authoritative for prepared media, retained source/variant bytes,
  thumbnail cache ownership, and private-data-free diagnostics.
- React owns only the ephemeral blob URL used to display an already prepared
  preview. It must revoke replaced and unmounted URLs exactly once.
- No timeline actor eviction, persistence format, or HEIF behavior change is
  part of this issue.

## Verify-first checks

The existing red regressions are the acceptance checks for the implementation:

- `koushi-media` Original/Keep must return the exact encoded source bytes while
  still validating dimensions and decoded-allocation limits.
- `koushi-core` renderable thumbnails must remain available through the existing
  129-entry churn contract while reporting bounded entry/byte statistics.
- The desktop preview test must revoke the active blob URL on unmount, while a
  caption-only rerender must not reload or revoke an unchanged preview.

## Implementation sequence

1. Add the non-HEIF Original/Keep dimension probe and exact-source fast path.
2. Replace the process-global thumbnail map with an explicit entry/byte LRU,
   including bounded, identifier-free eviction and clear diagnostics.
3. Add retained source/variant accounting, high-water marks, and cleanup
   diagnostics to `MediaPreparationRegistry`, with lifecycle coverage for item,
   target, thread, snapshot, account, and full clears.
4. Repair `PreparedUploadPreview` blob URL ownership with a ref-backed lifecycle
   that preserves the previous preview while a new variant is pending.
5. Record current thumbnail-cache and media-preparation summaries immediately
   before a user diagnostic export, keeping transport mapping in Tauri thin.
6. Reject a single over-bound thumbnail before publishing a Ready protocol URL.

## Acceptance and review record

- Focused results for this review pass: `koushi-media` image variants (15
  passed), `koushi-core` media preparation (10 passed), `koushi-core`
  renderable thumbnails (8 passed), the Tauri diagnostics export test (1
  passed), and the desktop dialogs Vitest file (13 passed). Desktop typecheck,
  lint, `cargo fmt --all -- --check`, and `git diff --check` also passed; stable
  rustfmt only reported the repository's existing nightly-option warnings.
- Diagnostics contain only bounded counts, sizes, stages, and fixed reason
  tokens; they never include URLs, filenames, room/user identifiers, or bytes.
