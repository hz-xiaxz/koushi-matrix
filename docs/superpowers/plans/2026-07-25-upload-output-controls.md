# Compact upload resize/format controls (issue #305)

Date: 2026-07-25

## Current shape (verified)

- `koushi-media::prepare_image_variants` decodes with the `image` crate and
  returns a fixed variant list (`original`, `resized-png`/`resized-jpeg`,
  `webp`) using `ImagePreparationPolicy::target_long_edge` and
  `quality_percent`. Encoding is Rust-side, not GUI-side.
- `koushi-core::media_preparation` caches `(target, staged_id, variant_id) ->
  CachedVariant { descriptor, bytes }` and projects
  `StagedUploadPreparation::Ready { variants, selected_variant_id }`.
- `koushi-state::PreparedUploadVariant` carries filename, mime, byte count,
  width/height, format, savings percent, and the metadata/thumbnail flags.
- `UploadStagingDialog` renders one large card per variant and calls
  `onSelectVariant(stagedId, variantId)` plus `loadPreview(stagedId, variantId)`.

So the encoder is already Rust-owned: #305 is a model change plus a GUI
redesign, not a move of pixel work into React.

## Target model

Resize and format become independent axes; a prepared variant is identified by
their combination.

```rust
pub enum PreparedUploadResize { Original, Half, Quarter, Eighth } // linear scale
pub enum PreparedUploadFormat { Keep, Png, Jpeg, Webp }           // Keep == source encoding

pub struct StagedUploadOutputSelection {
    pub resize: PreparedUploadResize,
    pub format: PreparedUploadFormat,
}
```

`StagedUploadPreparation::Ready` keeps `variants` as the completed-combination
cache and gains `selected: StagedUploadOutputSelection`, `pending:
Option<StagedUploadOutputSelection>`, and a `generation: u64` fence. The reducer
resolves a selection to a cached variant, or records it as pending so the GUI can
show `Recompressing…` without losing the last valid output.

`PreparedUploadVariant` gains `resize` so cache identity is the pair, and
`Original`/`Keep` stay distinct: resize `Original` preserves dimensions while
format `Keep` preserves the source encoding.

## Rust work

1. `koushi-media`: add a single-combination entry point that decodes once,
   applies the linear scale (halving per step, flooring at 1px, both dimensions),
   and encodes to the requested format; `Keep` re-encodes in the source format.
   Express `prepare_image_variants` in terms of it so there is one encoder path.
   Alpha: JPEG has no alpha, so flatten deterministically and record it.
2. `koushi-core::media_preparation`: key the cache by the pair, add a
   generation-fenced "prepare this combination" path, and drop stale results
   (latest selection wins). Reuse a completed combination immediately.
3. Typed command `SelectStagedUploadOutput { staged_id, resize, format }`
   replacing the opaque `variant_id` selection. Upload uses the selected
   combination's bytes.
4. Mirrors in the same change: `apps/desktop/src-tauri/src/dto.rs`,
   `apps/desktop/src/domain/types.ts`, the checked-in CoreEvent contract
   artifact, `browserFakeApi.ts`, `tauriIpcMock.ts`, `appHarnessMain.tsx`, and
   the DTO serialization-contract tests.

## GUI work

- One compact toolbar above the preview: a Resize radiogroup (Original, 1/2,
  1/4, 1/8) and a Format radiogroup (Keep, WebP, JPEG, PNG), each option
  ~24–28px inside a ~30–34px control, wrapping without becoming cards.
- Result summary at the toolbar's inline end: output dimensions, byte size,
  savings percent, and preparation state. No MIME strings anywhere.
- One full-width fixed-height preview viewport. While recompressing, keep and
  dim the current preview with a progress indicator; swap image, dimensions,
  size, and savings atomically; on failure keep the last valid preview, show a
  retry affordance, and do not silently change the selection.
- Caption field stays below the preview. Keyboard navigation, visible focus, and
  `aria-pressed`/radiogroup semantics preserved.
- `styles.css`: replace `.upload-staging-choice` / `.upload-variant-button` with
  the compact toolbar, summary, and preview-viewport rules; sizes behind named
  custom properties, not ad hoc `px` in TSX.

## Verification (RED first)

- `koushi-media`: resize math including odd dimensions and the 1px floor, each
  format encode, `Keep` fidelity, alpha flattening for JPEG, and exact output
  dimensions/byte counts.
- `koushi-core`: cache identity per pair, immediate reuse of a completed pair,
  generation fence dropping a stale encode, and upload using the selected bytes.
- Component: selection drives the typed command; stale results suppressed;
  loading keeps the preview mounted at a stable height; success swaps
  atomically; failure keeps the last preview; narrow layout wraps.
- Browser-headless: drive the real dialog controls and assert the recorded
  typed IPC plus the Rust-shaped snapshot rendering.
