# #1432: reader-scope avatar retirement and stale `Ready` projections

Status: implemented. Parent: [#1432](https://github.com/shinaoka/koushi-matrix/issues/1432).
Supersedes the scoped-demand retirement bullet of
[#1396](2026-10-13-issue1396-avatar-thumbnails.md).

## Defect

#1396 retired a source's retained bytes whenever it left the scoped
read-receipt reader demand (`AccountActor::reconcile_avatar_demand` diffed the
demanded sources and called `retire_avatar_source`). Leaving the reader window
does not prove the source is globally unused: the DM list, room header,
timeline, own profile, and receipt reader demand the same MXC through separate
paths. The bytes disappeared while AppState still projected `Ready`, so the
`koushi-thumbnail://` handler answered 404 and the avatar fell back to initials
until an unrelated read-receipt demand refetched the source.

## Change

- **No retirement on reader-scope exit.** `reconcile_avatar_demand` no longer
  diffs the demanded sources and `demanded_avatar_sources` is deleted. Retained
  bytes are released by the existing per-kind bounded LRU (8 MiB / 256 avatar
  entries) and the session lifecycle, which is what bounds the cache.
- **No removal may leave a silent stale `Ready`.** A `Ready` projection whose
  bytes are gone - LRU eviction, a superseded own avatar, or another account tab
  evicting under the shared budget - is downgraded to `NotRequested` at the two
  points where the actor can see it:
  - `cached_avatar_thumbnail` downgrades an entry it is asked for and cannot
    serve, so a revalidation that refetches (or is cancelled before the refetch
    completes) never leaves the projection pointing at missing bytes; and
  - `reconcile_avatar_demand` sweeps every other announced source whose bytes
    are gone, which covers entries no lookup reaches (a DM row, header, or
    profile copy the scoped demand does not reference).

  The reducer updates every matching copy (own/user/room-user/room/space/
  invite/receipt-reader avatars) and the renderer's existing visibility-driven
  demand re-requests the URI.
  - URIs the scoped reader demand still references are excluded from the sweep:
    they keep the observation-gated revalidation plus refetch path, so a working
    set larger than the renderable LRU still cannot drive an endless
    eviction/refill loop.
  - Availability is checked without touching LRU recency
    (`contains_renderable_thumbnail`), otherwise the sweep itself would reorder
    the cache and make eviction arbitrary.
  - `retire_avatar_source` therefore no longer erases the avatar-cache entry it
    superseded: the entry must stay visible to these downgrade paths.
- **Own-avatar retirement stays.** `note_own_avatar_source` is the one remaining
  explicit supersede signal and keeps #1396's P4 behavior. It no longer removes
  bytes silently: the next reconcile downgrades the superseded projection, and
  the self-healing path above applies.

## Non-goals

- No new renderer consumer ledger, and no demand registration for DM-list,
  header, or profile avatars; the issue's accepted alternative is the bounded
  cache and the session lifecycle.
- No change to per-kind budgets, lease accounting, the protocol handler, or the
  diagnostics counters (`retirement_count` / `retired_bytes` keep reporting the
  own-avatar path).
- Renderer-side decoded-bitmap eviction stays out of scope (#1396 P6).

## Verification

- `a_reader_scope_source_shared_with_another_visible_surface_keeps_its_bytes`
  (actor): one MXC is demanded by the reader scope and separately requested like
  a DM row; after the scope closes and a different source is demanded, the
  shared `source_ref` still resolves and the cached `Ready` still answers a
  repeat download without a second network fetch.
- `an_announced_avatar_whose_bytes_were_removed_is_downgraded_not_left_stale`
  (actor): evicting an announced source's bytes with synthetic cache pressure
  makes the actor emit `NotRequested` for it instead of leaving `Ready` pointing
  at a 404; a later download refetches and re-announces it.
- `a_demanded_source_whose_bytes_were_evicted_is_downgraded_before_its_refetch`
  (actor): a source the scoped demand still references is downgraded at the
  observation that revalidates it, so an aborted refetch cannot leave a `Ready`
  pointing at missing bytes.
- `an_invalidated_avatar_between_ready_and_not_requested_is_requested_again`
  (vitest, `Shell.test.tsx`): the renderer half of the invalidation path.
- `changing_the_own_avatar_retires_the_superseded_entry` keeps asserting byte
  retirement for the own-avatar signal.
