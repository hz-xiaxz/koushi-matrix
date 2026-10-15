# Avatar residuals: HEIF fitted-path coverage and the P6 keep-decoding decision (#1450)

Canon consulted: `REPOSITORY_RULES.md` ("Security Rules" — plaintext-derived
data and first-party stores; "QA Gates And Cleanup"), `AGENTS.md`, and the
governing plan `docs/plans/2026-10-13-issue1396-avatar-thumbnails.md`.

#1396 shipped with two residuals. Both are addressed here; neither needed a
state-machine or public-contract change, so no canon amendment was required.

## 1. HEIF through the fitted avatar path

`crates/koushi-media` decodes HEIF with the pure-Rust `heif_oxide`, and the
avatar pipeline calls `prepare_image_output_fitted(.., WebP, ..)`, but the HEIC
fixture only rode `prepare_image_output`. Added:

- `crates/koushi-media/tests/image_variants.rs::fitted_output_sizes_a_heif_avatar_and_reports_the_decoder_failure_kind`
  — runs the 64x64 `opaque.heic` fixture through the fitted path with the avatar
  policy (120 px, WebP) and asserts the output format, MIME type, dimensions,
  `fit-120-webp` identity and re-decodable pixels; also resizes the same fixture
  to a 32 px target, and asserts the decoder-guard failure kind
  (`ImagePreparationError::Decode`) for an oversized HEIF.
- `crates/koushi-core/src/account/profile_tests.rs::heif_avatar_content_reports_the_fetch_resolution`
  — runs the same fixture through `prepare_avatar_content` and asserts the
  `AvatarResolution` it reports (`LocalResize`), the WebP MIME type and the
  64x64 dimensions.

A platform without a HEIF decoder is not in scope: `heif_oxide` is a pure-Rust
dependency compiled on every target, so decoding is platform-independent. The
failure-kind assertion covers the decoder-guard path instead.

## 2. P6 — renderer-side decoded-bitmap eviction: keep decoding

`koushi-thumbnail://` continues to serve `Cache-Control: no-store`, so leaving
and re-entering a scope re-decodes the retained avatar bytes. Decision: keep
decoding, and record the measurement.

- **Privacy:** the served bytes are decrypted avatar/link-preview media.
  Making the response cacheable would let the WebView HTTP cache persist
  plaintext-derived data, which the Security Rules forbid outside the encrypted
  SDK store or encrypted search index (and which #1396's own "remove plaintext
  renderable thumbnail cache" predecessor removed from Rust).
- **Cost:** the retained avatar is a lossless WebP bounded by the 120 px long
  edge (400x200 → 120x60, 5,210 bytes). Decoding it takes ~112 µs per decode in
  release and ~1.38 ms in an unoptimized debug build, over 500 rounds of
  `image::load_from_memory_with_format` in a throwaway `koushi-media` test —
  far below one frame.
- **Compliant reuse would not be smaller:** it needs a renderer-side
  decoded-bitmap cache (JS memory, LRU, canvas rendering) at every avatar call
  site, because there is no shared avatar component. That is a larger mechanism
  than the residual justifies.

The `no-store` response stays pinned by
`apps/desktop/src-tauri/src/tests.rs::renderable_thumbnail_protocol_serves_known_cached_bytes`;
its purpose is recorded in a one-line comment on the response builder. The
decision and measurement are recorded in the plan's Residual section.

## Verification

- `cargo test -p koushi-media` (includes the new fitted-path test and the whole
  fixture matrix)
- `cargo test -p koushi-core --lib` (includes the HEIF avatar-content test)
- `cargo test -p koushi-desktop --lib` (protocol response header)
- `cargo fmt --check`, clippy `-D warnings` for the three touched crates
