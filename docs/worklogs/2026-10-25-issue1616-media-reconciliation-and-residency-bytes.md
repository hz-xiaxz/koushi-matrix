# #1616: media-source reconciliation and residency byte diagnostics

Follow-up to the ownership review in
`2026-10-25-issue1616-memory-code-review.md`. Two changes: retire stale
`media_sources` metadata, and add a retained-byte lower bound to the residency
diagnostics so the app-side share of the live heap can be bounded.

## Media-source reconciliation

`apply_sdk_media_cache_diff` (`crates/koushi-core/src/timeline/media.rs`) only
added entries. Removal diffs carry no event ID, so removed media events kept
their metadata, and a `Set` diff that edited media away (`m.image` to text) kept
a stale entry that no longer had a resident projection.

- `cache_sdk_item_media_source` now removes the event's entry when the new item
  carries no media, so `Set`/`Push*`/`Insert` clear stale metadata.
- After a batch that contains `Remove`/`Truncate`/`PopFront`/`PopBack`, the relay
  reconciles the map against the committed `navigation_items`
  (`retain_resident_media_sources`), keeping entries whose download is still in
  flight because the worker and progress events still read them.
- `Reset`/`Clear` behavior is unchanged, and authoritative recovery still
  replaces the whole map (`replace_authoritative_cache`).

The map is metadata only (`PrivateMediaEntry` holds a source, mimetype, size and
dimensions), so this is bounded lifetime hygiene, not a large byte win.

## Residency byte diagnostics

`TimelinePositionIndex` now carries two lower bounds, computed where the index
already walks the items (#1394):

- `resident_item_struct_bytes`: `size_of::<TimelineItem>()` per resident item;
- `resident_item_text_bytes`: `body`, formatted `html` and `plain_text` bytes.

`TimelineResidencyDiagnostics` sums them into
`resident_item_struct_bytes` / `resident_item_text_bytes`, and the diagnostics
export records both on the `core.timeline_residency` row. Both are lower bounds:
heap shared by several owners, derived projections and the SDK's own copy of the
event are not counted.

## Deliberately unchanged

- `thread_unread` lifetime (cleared on confirmed leave only).
- The #1238 diagnostic thread subscription (it is also the mechanism that fills
  the SDK thread cache for unopened threads).
- `timelineViewportSessionMemory` (documented per-session UI memory).

## Verification

- `cargo test -p koushi-core --lib -- timeline::media timeline::diagnostics`
- `cargo test -p koushi-desktop --lib -- diagnostics::snapshot_tests`
- `cargo clippy -p koushi-core --tests`

Still unmeasured: the SDK event-cache chunk bytes. The next step remains a
vendor `memory_diagnostics` byte estimate or stack attribution, not another
app-side counter.
