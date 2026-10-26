# Timeline media downloads: opaque refs instead of cache paths (#1619)

Consulted REPOSITORY_RULES.md, AGENTS.md, the media download contract in
`docs/architecture/state-machine.md`, the media ownership rules in
`docs/agents/state-ownership.md`, `docs/architecture/overview.md`, and the
verification guidance. The maintainer chose the avatar-style opaque-reference
design over a byte-count-only DTO so rendered downloads keep working; the canon
amendment was approved before implementation.

## Problem

`MediaDownloadCompleted` and `TimelineMediaDownloadState::Ready` carried
`source_url`, the absolute path of the managed cache file. The event and every
snapshot therefore exposed the local account layout to React and QA output, and
the save command accepted a renderer-supplied path. The documented contract said
downloads expose byte counts only and that paths never enter Core state,
events, or commands.

## Design

- Core names the managed entry `download/<room-hash>/<event-hash>`, built from
  the same hashed components that already name the cache directory. The ref has
  no path segment and validates as exactly two decimal hashes of at most 16
  digits.
- `MediaDownloadCompleted` and `Ready` now carry `source_ref`, `byte_count`, and
  the existing safe media metadata (mime type, dimensions). Core keeps the
  `PathBuf` actor-private for the existing download/save call sites.
- The Tauri adapter resolves refs against the account cache roots:
  `koushi-media://` serves image bytes with an allowlisted Content-Type and a
  size cap, while `save_downloaded_media` resolves the ref before invoking the
  Core save policy. The renderer never names a filesystem path.
- A cached download without event mime metadata sniffs the image header (32
  bytes) so the protocol can pick a Content-Type; unknown types fall back to the
  plain download card instead of an empty `<img src>`.
- CSP and the release preflight allow `koushi-media:` alongside the existing
  thumbnail protocol.

## Verification

- `cargo test -p koushi-core media_download` (ref validation, debug redaction,
  wire serialization).
- `cargo test -p koushi-state media_download` and the state snapshot test.
- `cargo test -p koushi-desktop media_download` (managed-cache serving, traversal
  and symlink escape rejection).
- `cargo test -p koushi-desktop core_event_wire_format_matches_checked_in_contract_artifact`
  after regenerating `coreEvents.generated.json`.
- `cargo fmt --check` and
  `cargo clippy -p koushi-core -p koushi-state -p koushi-desktop --all-targets -- -D warnings`.
- `npx tsc --noEmit`, `npm run lint`, `npm run lint:rust-test-structure`.
- `npx vitest run` (2294 tests) and Playwright chromium
  `media-attachments.spec.ts`, `timeline-scrollback.spec.ts`, and
  `room-space-invites.spec.ts`.
