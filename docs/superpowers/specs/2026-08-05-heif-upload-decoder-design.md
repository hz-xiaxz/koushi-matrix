# HEIC/HEIF Upload Decoder Design

**Issue:** #426

## Goal

Decode supported HEIC/HEIF still images into the existing bounded image
preparation pipeline so users can preview and convert them to JPEG, WebP, or
PNG while retaining the exact original bytes as an explicit fallback.

## Mandatory Decoder Selection Gate

Implementation begins with a documented dependency evaluation, not a decoder
commit. Compare viable Rust/native-backed decoders using reproducible builds on
macOS, Windows, and Linux; supported ISO BMFF brands and HEVC profiles;
orientation, color profile, alpha, HDR, and primary-image behavior; static and
dynamic packaging requirements; binary-size change; license and patent terms;
maintenance health; and CI fixture reproducibility.

Reject candidates that depend on optional end-user OS codecs, cannot enforce
the repository's allocation limits, or cannot be distributed consistently on
all supported targets. If no candidate passes, retain generic original upload
and close the implementation phase as blocked with the evaluation evidence.

## Architecture

Add the selected decoder only inside `koushi-media`, behind a small internal
function that converts recognized HEIF input into the same bounded pixel
representation consumed by the existing resize and PNG/JPEG/WebP encoders.
Reuse existing output identities, cache behavior, quality policy, metadata
stripping, thumbnail regeneration, staging DTOs, retry, encryption, and upload
paths. Do not add a parallel HEIF staging state machine.

Content recognition parses a bounded ISO BMFF `ftyp` header and treats MIME and
extension as hints. It reports a normalized detected brand and rejects
malformed containers before decode. Existing decoded-dimension and allocation
limits apply before producing a pixel buffer.

The decoder selects the primary still image, applies orientation, converts to
defined sRGB pixels, and exposes alpha/HDR facts needed by the existing output
recommendation. Opaque SDR photos recommend JPEG; alpha recommends WebP or
PNG. Unsupported sequences, auxiliary-only images, profiles, and HDR/gain-map
content without a tested tone mapper return a typed preparation failure while
leaving Original available. Live Photo video data is ignored and never copied
into a converted output.

Converted upload metadata is derived from the prepared output, so extension,
Content-Type, Matrix `info.mimetype`, dimensions, byte count, thumbnail, and
encoded bytes agree. Original selection preserves exact bytes and a normalized
HEIC/HEIF MIME type.

## Diagnostics And Privacy

Diagnostics expose only source classification, detected brand, decoder name,
coarse outcome, dimensions, alpha/HDR/orientation booleans, output format and
counts, and metadata/thumbnail completion booleans. They never expose filenames,
bytes, EXIF values, thumbnails, Matrix IDs, or user content.

## Verification

The dependency gate records build and package smoke results on all supported
targets and measured artifact-size deltas. Implementation starts with licensed
or generated fixtures for opaque, rotated, malformed, oversized, MIME-mismatch,
unsupported-profile, and—when supported—alpha inputs. Tests prove exact
original preservation; JPEG/WebP/PNG byte and metadata agreement; encrypted
and unencrypted preparation/retry parity; safe diagnostics; and unchanged
PNG/JPEG/WebP behavior. Long local homeserver and platform packaging gates run
once after focused media, core, Tauri, TypeScript, and browser tests pass.
