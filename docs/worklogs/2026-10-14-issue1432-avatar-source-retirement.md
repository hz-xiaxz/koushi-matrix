# #1432: reader-scope avatar retirement left announced thumbnails stale (2026-10-14)

## Evidence

- Headless actor regressions failed on the pre-fix code:
  `a_reader_scope_source_shared_with_another_surface_keeps_its_bytes` (a source
  demanded by the reader scope and independently by a DM-row-style
  `DownloadAvatarThumbnail` lost its retained bytes when the reader scope moved
  on) and `an_announced_avatar_losing_its_bytes_is_downgraded_not_left_stale`
  (an announced `Ready` whose bytes were evicted stayed `Ready` and never
  produced a `NotRequested` downgrade).
- The removed code was #1396's diff of `demanded_avatar_sources` against the next
  reader scope, which called `retire_avatar_source` for every source that left
  the visible-plus-prefetch window. A source can be shown by a DM row, room
  header, timeline copy, or the own profile without being in that window.

## Change

- `reconcile_avatar_demand` no longer retires bytes when a source leaves the
  scoped demand; `demanded_avatar_sources` and its session bookkeeping are
  deleted. The per-kind bounded LRU (8 MiB / 256 avatar entries) and the session
  lifecycle bound the cache.
- Every reconcile now downgrades an announced avatar whose retained bytes are
  gone and which the scoped demand does not reference, and
  `cached_avatar_thumbnail` downgrades a source it is asked for and cannot
  serve: the actor drops the avatar-cache entry and emits
  `AvatarThumbnailUpdated { mxc_uri, thumbnail: NotRequested }`. The reducer
  updates every matching copy and the renderer's visibility demand re-requests
  the URI.
- Availability is checked with a new non-touching
  `contains_renderable_thumbnail` for the sweep; the existing
  `is_renderable_thumbnail_cached` still refreshes recency on the demand path.
- `retire_avatar_source` (own profile moved to a new avatar) still releases the
  superseded bytes, but no longer erases the avatar-cache entry silently, so the
  same downgrade covers it.
- Demanded resources keep the existing `cached_avatar_thumbnail` revalidation
  and refetch path, which is the guard against an endless eviction/refill loop
  when the working set exceeds the renderable LRU.

## Verification and scope

The three actor regressions failed before the change and pass with it (the
shared-source one after #1432's original code, the two downgrade ones before
their respective downgrade site existed); the frontend guard
`requests an icon again after Rust downgrades a ready thumbnail`
(`Shell.test.tsx`) covers the renderer half of the downgrade path, and
`changing_the_own_avatar_retires_the_superseded_entry` and
`scoped_avatar_rehydrates_evicted_ready_bytes_from_sdk_cache` keep their
contracts. Ran `cargo test -p koushi-core --lib` (1604 passed, 10 ignored, 10
repeats to check stability), `cargo test -p koushi-core --test media_save --test
link_preview`, `npm --prefix apps/desktop test -- src/components/Shell.test.tsx`
(37 passed), `npm --prefix apps/desktop run typecheck`, `cargo fmt --check`, the
workspace/QA/release clippy gates, and `node scripts/check-agents-docs.mjs`.

The local suite has a pre-existing flaky test that is unrelated to avatars:
`avatar_actor_bounds_distinct_demand_and_rejects_capacity` failed once in 25
`koushi-core --lib` runs on `origin/main` and twice in 10 on this branch; it
times out waiting for a media request whose 5 s delayed response never lands.
It is left untouched here.

AC4's native GUI confirmation (DM list, header, timeline, and reader popup
avatars visible across reader-scope changes) is a manual check and is not
replaced by these headless tests. Two boundaries remain and are tracked
separately: a source the scoped demand still references and whose bytes were
evicted is downgraded only at its next observation or lookup, and a *capacity*
eviction of a visible source can be re-requested by the renderer while a working
set exceeds the byte budget. Invalidating demanded sources on every completion
reconcile is exactly the eviction/refill loop the demand guard prevents, so that
case needs a capacity policy of its own.

Consulted `REPOSITORY_RULES.md` (Root-Cause Fix Discipline, Canon-First Change
Protocol, Review And Audit), `docs/architecture/overview.md` (the renderable
thumbnail cache), `docs/agents/verification.md`, `docs/agents/state-ownership.md`
(profile/avatar ownership), and the #1396 plan. The overview's cache paragraph
now states the release and downgrade contract. No user-guide text changes are
needed: no label, menu path, or user-visible instruction changed.
