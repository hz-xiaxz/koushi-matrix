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

## Target state model

`PreparedUploadFormat` keeps describing the *actual* encoding of a prepared
output. Selection gets its own types so `Original` (dimensions) and `Keep`
(encoding) cannot be confused:

```rust
pub enum StagedUploadResizeChoice { Original, Half, Quarter, Eighth }
pub enum StagedUploadFormatChoice { Keep, Png, Jpeg, Webp }
pub struct StagedUploadOutputSelection { resize: …, format: … }
```

`PreparedUploadVariant` gains `resize` and `format_choice` so the GUI matches a
selection to a cached output without parsing `variant_id`.

`StagedUploadPreparation::Ready { variants, selected, pending, generation }` has
exactly one owner of "which output": `selected`. `variants` is the completed
cache; a selection whose pair is absent is simply not prepared yet, which is
what `pending` reports. `selected_variant_id` is removed — two fields that must
agree would be duplicate state.

While `pending` is set, React keeps the preview image it already loaded (that is
presentation state it owns) and the summary shows the preparation state instead
of stale numbers, so displayed dimensions and byte size never describe anything
other than the bytes that would be uploaded.

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

## Decided: always ask, default scale 1

Maintainer decision (2026-07-25): the staging dialog always asks. There is no
automatic compression path for staged uploads.

- The initial selection is always `(Original, Keep)` — scale 1, source encoding.
- The user then explicitly picks `1/2`, `1/4`, or `1/8`, and a format.
- Aspect ratio is fixed: both dimensions are divided by the same divisor, which
  is what `scale_linearly` already does (integer division, floored at 1px).

Why not keep the existing setting-driven behavior: `prepare_image_variants`
compresses by long edge (2048), so its output is neither `Original` nor `1/2`
for a given image (4000x3000 becomes 2048x1536 ~ 0.512; 1284x918 is left
untouched). Labelling that output as one of the four choices would be false, and
keeping it beside axis-based outputs would preserve exactly the opaque-variant
model #305 removes. Mapping `Always` onto `(Half, Keep)` was rejected because it
would newly shrink images the long-edge policy deliberately left alone.

Consequence to carry out with the implementation: `SettingsValues.media
.image_upload_compression` stops influencing staged upload preparation. A
setting that no longer changes behavior must not stay visible, so its settings
UI, DTO/TS mirrors, persistence backfill, the image-compression Rust tests, the
`image_compress=ok` core token, and the `local-image-compression` Linux lane all
need to be retired or repointed at the dialog in the same change. Treat that as
part of #305 scope, not a follow-up, so no dead preference remains.

## Retirement scope (verified 2026-07-25)

`media` holds two separate things, and only one of them is inert:

- `image_upload_compression: ImageUploadCompressionMode` (`Never`/`Ask`/
  `Always`) no longer affects anything: staging always asks and always starts at
  `(Original, Keep)`. **Retire it** — the enum, the `SettingsValues` field, the
  `SettingsPatch` entry, the User settings control and its catalog keys, the
  DTO/TS mirrors, and every fixture that sets it.
- `image_upload_compression_policy` (threshold/target/quality) is still live:
  `prepare_image_output` reads `quality_percent` for JPEG/WebP encoding, and
  `build_upload_media_command` still reads the thresholds. **Keep it**, but
  rename away from "compression" if it survives the UI work, since it is now an
  encoder policy rather than an automatic-compression policy.

The `local-image-compression` Linux lane drives the retired control ("set
Compress images to Always"), so it must be repointed at the staging dialog:
attach the synthetic wide PNG, pick `1/2` and `JPEG`, and assert the Rust-owned
media row reports the selected output. Do not simply delete the lane; it is the
only virtual-display proof that a chosen output is what actually uploads.

Order the commits UI-first: the toolbar has to exist before the old control is
removed, otherwise the tree has a moment with no way to choose compression at
all.

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
