# #1396: avatar thumbnails — server size, local fallback, identity, budget

Status: implemented. Parent: [#1396](https://github.com/shinaoka/koushi-matrix/issues/1396).
The issue supplied the design; this record states what landed, the two decisions
the issue left open, and what was measured.

The avatar path downloaded the original upload and retained it: 86 entries at
246 KiB average (21.7 MiB) under a shared 32 MiB avatar+link-preview cap, while
the largest avatar slot is 40 CSS px.

## What landed

- **P1 — server thumbnail.** `download_avatar_thumbnail` asks for
  `MediaFormat::Thumbnail(MediaThumbnailSettings::new(120, 120))`, sized from the
  40 px slot at 3x DPI, exactly as timeline media does.
- **P2 — bounded local fallback.** Only a failed thumbnail request downloads the
  original, and `koushi_media::prepare_image_output_fitted` resizes it to the
  long edge rather than a linear fraction (`resize_to_long_edge` was private and
  only reachable from `prepare_image_variants`). It never upscales.
- **P3 — no retained original.** Every decodable result is re-encoded (lossless
  WebP for all fetched sources, server thumbnail included), so what enters the
  cache is within the pixel budget and carries its dimensions.
- **P4 — identity and invalidation.** The cache key includes the thumbnail
  identity (`source` + `fit-120-webp`), not only kind/account/source, so two
  sizes cannot collide. A superseded source retires its bytes through
  `retire_avatar_source`, driven only when the own profile moves to a new avatar
  (`note_own_avatar_source`). [#1432](../plans/2026-10-14-issue1432-avatar-source-retirement.md)
  removed the scoped-demand half of this: leaving the reader window does not
  prove the source is unused, because a DM row, header, timeline copy, or own
  profile can demand the same MXC outside that scope. The bounded cache and the
  session lifecycle own that release instead.

  A renderer cancel is deliberately **not** a retirement signal: the renderer's
  request bridge cancels as soon as an avatar becomes `Ready` (the snapshot
  planner drops a URI once it leaves `notRequested`, and the viewport effect's
  cleanup runs on the same transition), which is not the end of its display
  lifetime. Retiring there deleted the just-loaded avatar's bytes and 404'd a
  visible image. The same reasoning applies to the scoped demand: it is a
  window, not ownership.
- **P5 — per-kind budgets.** Avatars: 8 MiB total, 256 entries, 256 KiB
  per-entry; link previews keep 32 MiB / 32 MiB per-entry. The two no longer
  compete.
- **P7 — measurement.** `core.renderable_thumbnail stage=summary` reports the
  per-kind split (`avatar_*`, `link_preview_*`), `average_entry_bytes`,
  `retirement_count`/`retired_bytes`, and the per-kind caps; resolution counters
  `avatar_resolution_{server_thumbnail,local_resize,original_passthrough,failure}`
  count once per stored avatar and once per terminal failure (a retried fetch
  that finally succeeds counts a failure and then its resolution).

## Decisions the issue left open

- **Same-URI content replacement.** The cache trusts the URI for the session;
  a re-upload that reuses a URI is not detected, because Matrix media content is
  immutable by contract and a re-upload yields a new URI. Session change still
  clears the account scope (`clear_renderable_thumbnail_cache_for_account`), and
  the LRU bounds any residue.
- **Non-decodable content.** GIF and animated WebP/PNG cannot be resized and
  report `ImagePreparationError::UnsupportedFormat`; their original bytes are
  kept verbatim, bounded by the 256 KiB avatar per-entry cap. A *decoder guard*
  rejection (dimension/allocation limit) stays `Decode` and is a failure, so a
  rejected image never becomes a pass-through. Oversize content is rejected and
  the avatar settles as failed rather than silently retaining an original. The
  resolution counter distinguishes the two. HEIF decodes through the same fitted
  path.

## Measured

Before, from the issue's 13.6 h session: 86 entries, 21,712,039 bytes retained,
246 KiB per entry, no eviction, shared 32 MiB cap (65% full).

After, from the new tests: a synthetic 400x200 avatar becomes a 120x60 lossless
WebP of 5,210 bytes; a 64x32 source is kept at its own size; the avatar budget is
8 MiB with a 256 KiB per-entry cap, separate from link previews.

Pending: a real-session `retained_bytes` / bytes-per-entry / WebContent-footprint
pair, which needs a packaged build and the same 13.6 h shape. The counter split
above is what makes that comparison readable from one export.

## Residual

Renderer-side decoded-bitmap eviction (P6) — decided 2026-10-15 (#1450): keep
decoding, do not add a renderer bitmap cache.

`koushi-thumbnail://` serves `Cache-Control: no-store`, so leaving and
re-entering a scope re-decodes the retained bytes. Those bytes must not become
cacheable: they are decrypted avatar media, and `REPOSITORY_RULES.md`
("Security Rules") prohibits persisting plaintext-derived data in a first-party
store such as the WebView HTTP cache. A compliant reuse would need a new
renderer-side decoded-bitmap cache (JS memory, LRU, canvas rendering) at every
avatar call site — a larger mechanism than the residual is worth.

Measured cost of the decode a reuse cache would avoid: the retained avatar is a
lossless WebP bounded by the 120 px long edge (400x200 → 120x60, 5,210 bytes);
decoding it with the `image` crate takes ~112 µs per decode in release and
~1.38 ms in an unoptimized debug build, over 500 rounds — far below one frame.
The `no-store` header stays pinned by
`apps/desktop/src-tauri/src/tests.rs::renderable_thumbnail_protocol_serves_known_cached_bytes`,
which is the headless check that re-entry re-fetches instead of serving a
cached response. Reproduce the measurement with a throwaway
`cargo test -p koushi-media --release` timing loop over
`prepare_image_output_fitted(.., 120, WebP, ..)` output.

Link-preview sizing beyond the budget split is out of scope.

## Verification

- `cargo test -p koushi-media` — fitted resize, no-upscale, non-decodable
  reporting, header dimension probe.
- `cargo test -p koushi-core --lib` — cache identity, source retirement,
  independent per-kind budgets, avatar content sizing, the release-retires-bytes
  actor path, and the updated receipt/lease tests.
- `cargo test -p koushi-desktop --lib` — the thumbnail summary export.
- Rust lint gate and the workspace suites as required by
  [verification](../agents/verification.md).
