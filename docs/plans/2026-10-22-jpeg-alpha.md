# Image conversion fidelity

## Problem and scope

JPEG conversion discarded alpha with `to_rgb8`, exposing hidden RGB in
transparent margins. The defect existed in `20599d5a`; the exact recent trigger
is not established. The format audit also reproduced RGB bleeding during
alpha-preserving resizes, discarded EXIF orientation, and discarded ICC profiles.
All fixtures and output comparisons are synthetic; no supplied screenshot or
private content is stored in the repository or QA artifacts.

Canon consulted: `REPOSITORY_RULES.md` (root-cause fixes, verify first, privacy,
review), `docs/agents/verification.md`, `docs/policies/engineering-rules.md`, and
the staged-upload and native-image sections in `docs/architecture/overview.md`.

The module-local policy clarifications are accepted by self-review: JPEG
composites over white before filtering; PNG/WebP resample premultiplied alpha;
ordinary raster decoders apply EXIF orientation before stripping metadata;
RGB ICC profiles travel with the encoded color channels. Non-RGB profiles use
the existing original-file fallback. No crypto, authorization, unsafe/FFI,
transport, state-machine, dependency, or public decoder-port change is needed.
Independent review is not required for this risk scope.

## Implementation and production path

- Desktop media staging calls Core's `MediaPreparationService::encode_output`,
  which dispatches ordinary images to `koushi_media::prepare_image_output`.
  Initial preparation and legacy variants use the same encoders. The chosen
  output's bytes, MIME type, filename and oriented dimensions reach the existing
  registry and send path; no renderer-side conversion is added.
- `crates/koushi-media/src/lib.rs` owns white JPEG compositing, alpha-aware
  resampling, EXIF normalization, oriented header-only dimension probes, and RGB
  ICC retention. The native HEIF path already supplies upright SDR sRGB pixels;
  no second rotation or original-profile attachment is applied there.
- History-export thumbnails use the same compositing and ICC rules in
  `src/thumbnail.rs`. HEIF's JPEG variant composites the source before its own
  resize, while its PNG/WebP variants retain alpha.
- Unscaled Original+Keep stays byte-identical, including source metadata. Its
  descriptor reports oriented display bounds. Transformed outputs strip EXIF,
  XMP and unrelated metadata but retain compatible RGB ICC rendering metadata.
- The user guide explains JPEG's white background, alpha-preserving alternatives,
  display orientation, color profiles and the original-file fallback.

## Verify-first evidence

- Before the JPEG fix, `cargo test --locked -p koushi-media --test jpeg_alpha`:
  six failures, one preservation check passing. Transparent margins decoded as
  black. The same seven tests pass after the implementation.
- Before the alpha/orientation fix, `--test image_transforms`: three failures,
  one preservation check passing. Altering only invisible source RGB affected
  236 visible pixels in a 32x32 resize (maximum channel difference 24/255), and
  a 40x20 image tagged EXIF orientation 6 stayed 40x20 instead of upright 20x40.
  The permanent tests cover all eight orientations, all three raster input
  formats, all four output choices and all four resize choices, visible colors
  on light/dark backgrounds, fitted/native outputs, and exact original bytes.
- Before ICC retention, `--test image_color_profiles`: two failures. The tests
  cover compatible profile bytes and visible pixel channels across every
  source/output/scale combination, legacy variants, fitted outputs, thumbnails,
  and the unsupported-profile original-file fallback. The 588-byte sRGB fixture
  is generated data with a fixed date and its own provenance note. A valid still
  WebP whose ICC manufacturer signature contains `ANIM` also reproduced a false
  animation rejection; the fixed RIFF chunk parser passes that regression and
  still retains a genuine two-frame animated WebP without frame loss.
- Existing HEIF/native-decoder, corrupt/oversized input, animation-preservation,
  MIME/filename, metadata, and retention tests remain part of the media suite.

## Validation and review

- Focused media suite: 49 tests passed; Clippy's checked-division finding was
  corrected without a lint suppression.
- Frontend: typecheck, lint and all 2,251 Vitest tests passed.
- Full workspace, Rust lint/stack gates, local Tuwunel/Synapse QA, documentation
  checks, and PR CI results are tracked in the validation section of PR #1582
  before merge; this worklog records the focused verify-first evidence.
- Self-review traces production initial/lazy/native/thumbnail callers, keeps
  public contracts unchanged, and checks new test files and the guide alongside
  the complete diff. No account content is used in any review material.
