# Issue #761 — Rust-Owned Preferences And TypeScript Semantic Deletion

Status: approved for implementation.

Base: `origin/main` `c7b5fd171f93191c039f1c137084a3d50601b444`.

Selected independent reviewer: Fireworks `reviewer-flash` (user-selected, read-only, different model family).

## Outcome

Complete #761 in one independently mergeable PR:

1. Rust owns every persisted user-visible preference currently stored by the WebView.
2. Existing user-authored values migrate once from the seven legacy `localStorage` families without entering logs or the non-secret settings file when they contain Matrix identifiers or free-form private text.
3. Rust projects complete sidebar section membership and ordering; React only filters the already ordered visible list by the mounted search draft.
4. Timeline search highlighting renders Rust-provided UTF-16 ranges; JavaScript no longer reimplements the Rust NFKC/case-fold matcher.
5. Browser tests use explicit Rust-shaped snapshots/events and a transport mock. `BrowserFakeApi` and its broad product state machine are deleted.
6. The production `DesktopApi` loses fake-only methods; no generic command framework replaces the deleted code.

DOM focus, unacknowledged text, popover/dialog state, sidebar filter draft, emoji-picker category/roving focus, viewport measurement, and visual collapse animation remain renderer-owned. Name sorting for presentation-only collections, mounted text filtering, date/gap row construction, enum-to-label rendering, and visibility-demand detection may also remain in TypeScript; none may reclassify Matrix/product semantics or reorder Rust-authoritative sidebar vectors.

## Recon Evidence

At the pinned base:

- `apps/desktop/src/app/localPresentation.ts` owns four keys:
  `koushi.spaceLocalOverrides.v1`, `koushi.displayDensity.v1`,
  `koushi.sidebarRoomCategory.v1`, and `koushi.sidebarRoomSort.v1`.
- `apps/desktop/src/App.tsx` owns `koushi.homeSelection.v1`.
- `apps/desktop/src/components/Shell.tsx` owns
  `koushi.roomSectionCollapsed.v1`.
- `apps/desktop/src/components/EmojiPicker.tsx` owns
  `koushi-recent-emojis` (bounded to 24 only in TypeScript).
- `desktopModel.ts::{roomListSections,composeSidebar}` plus
  `Shell.tsx::sortedSidebarRooms` duplicate Rust sidebar classification,
  attention, section, scope and sort behavior.
- `components/searchHighlight.ts` explicitly mirrors Rust NFKC/case-fold
  matching.
- `backend/roomListProjection.ts`, `backend/browser-fake/sidebar.ts`,
  `backend/browser-fake/settings.ts`, and `browserFakeApi.ts` duplicate Rust
  room-list, settings/profile, composer resolver, search, navigation, and many
  other state transitions.
- `BrowserFakeApi` is 5,719 lines; `DesktopApi` declares 165 methods. The
  interface includes the fake-only `setRoomListProjection`, whose production
  implementation is an empty method.
- Existing focused baseline is green:
  `cargo test -p koushi-state --test settings_state --test navigation_state`
  and 276 Vitest cases across Shell, EmojiPicker, desktopModel,
  BrowserFakeApi and client.

## Canon Reconciliation

The issue says product/user-visible authority moves into `SettingsValues`, but
current privacy canon forbids Matrix IDs and arbitrary user-authored text in
`settings/settings.json`. Preserve that stricter rule:

- device-global, typed, non-secret preferences move into `SettingsValues`;
- Home DM selection and per-Space local name/icon overrides move into the
  existing per-account encrypted `NavigationState` store
  (`navigation/navigation.v1.enc`);
- React never combines an override with an upstream label. Rust projects the
  final Space rail/header label and optional local icon;
- free-form Space override content and Matrix IDs use redacted `Debug` wrappers.

This is not a compatibility shim. It is the required data-preserving migration
from the last supported WebView keys. Its live consumer, deletion condition and
proof are recorded below.

## Data Contract

### Global settings

Extend existing structs rather than add a parallel store:

```rust
AppearanceSettings {
    theme: ThemePreference,
    density: DisplayDensity, // compact | default | comfortable
}

ComposerSettings {
    math_mode: bool,
    recent_emojis: Vec<String>, // canonical distinct MRU, max 24
}

SidebarSettings {
    category: SidebarCategory, // rooms | people
    collapsed: SidebarCollapsedSections,
}

SettingsValues {
    ...,
    sidebar: SidebarSettings,
    room_list_sort: RoomListSort,
    legacy_frontend_preferences_imported: bool,
}
```

Defaults preserve current behavior: comfortable density, Rooms category,
Activity sort, no collapsed sections, empty recent emoji list. Legacy JSON uses
`#[serde(default)]`. `SettingsValues::apply_patch` canonicalizes recent emoji
values (non-empty, bounded scalar length, stable dedupe, max 24) so a WebView
cannot persist an unbounded vector.

The existing `UpdateSettings` path remains the normal user mutation. One
migration-only `ImportLegacySettings` command accepts the same typed patch but
is rejected only for the `SettingsStore::load` error / `SettingsLoadFailed`
branch, preventing a corrupt/unreadable existing file from being overwritten by
browser defaults. `NotFound -> Ok(SettingsValues::default())` remains eligible
for a fresh-install legacy import. AppActor retains only a private load-status
enum; no second settings state machine enters the snapshot. Import clones the
current values, applies/canonicalizes the patch, sets the persisted import marker,
saves that clone first, and projects it only after save success. If the marker is
already true, the command is a benign no-op and never reapplies stale WebView
values. Sidebar sort maps old `active` to `Activity` and old `name` to
`NormalLocale`.

### Account-private navigation preferences

Extend `NavigationState`, which already uses the account-keyed encrypted store:

```rust
NavigationState {
    ...,
    legacy_frontend_preferences_imported: bool,
}
HomeSelection = Activity | Explore | Invites | DirectMessage { room_id }
SpaceLocalPresentation { name: Option<String>, icon: Option<String> }
SpaceLocalPresentations(BTreeMap<String, SpaceLocalPresentation>)
NavigationPreferenceUpdate =
    SetHomeSelection { selection }
  | SetSpacePresentation { space_id, presentation }
  | ImportLegacy { home_selection, space_presentations }
```

Core validates limits before reducer admission: at most 256 retained Space
entries, 128 Unicode scalars for a name and 12 for an icon, trims blank values,
and removes empty overrides. Unknown-but-well-shaped Space IDs are retained so
data does not disappear merely because Sliding Sync has not projected that
Space yet. An unavailable remembered DM falls back to Activity when opened,
matching current behavior, while the encrypted preference remains available
for later projection. Replace `NavigationState`'s derived `Debug` with a custom
whole-state redacted implementation: it exposes booleans/counts and coarse enum
kinds only, never any current/remembered room/Space/event ID, local Space
name/icon, or scroll-anchor identity. `HomeSelection` and the Space-presentation
wrapper are redacted independently as defense in depth. RED tests format the
complete `state.navigation` and require every synthetic identifier/name/icon to
be absent.

One typed `update_navigation_preference` command carries the closed update enum.
It is Ready-session/account scoped. AppActor first calls the existing
`load_navigation_for_current_session` barrier and verifies that
`navigation_loaded_for` plus `NavigationPersistenceStatus` match the current
`SessionKeyId`; therefore an import can never be overwritten by a later startup
load. Direct user edits project one reducer action and persist through the
existing deferred path. Import instead works on a clone, sets its persisted
marker, saves the encrypted clone first, and only then projects it through
`NavigationLoaded`; persistence failure leaves both live state and marker
unchanged. An already-set marker makes replay a benign no-op. All variants use
custom redacted `Debug` for identifiers and free-form values. A load failure
rejects the import without deleting legacy keys. A later direct user edit may
use the existing explicit-mutation recovery rule, but migration import itself
never overwrites a store whose prior contents could not be loaded.

### Sidebar projection

Add `SidebarSections` to `SidebarModel`:

```rust
SidebarSections {
    favourites: Vec<RoomListItem>,
    rooms: Vec<RoomListItem>,
    people: Vec<RoomListItem>,
    low_priority: Vec<RoomListItem>,
    not_joined: Vec<RoomListItem>,
}
```

Add `local_icon: Option<String>` to `SpaceRailItem`. A new
`compose_sidebar_for_state(&AppState)` is the production composition seam. It
uses the current active Space, notification facts, invites, settings sort, room
tags/DM classification, and encrypted local Space presentation. Existing
fact-only wrappers remain only where a caller genuinely lacks `AppState`; they
must not be used by production Tauri/state-delta paths.

All section vectors are ordered in Rust by the selected `RoomListSort`, with the
same attention/activity comparator and deterministic label/id fallback used by
`RoomListProjection`. Account-global invites do not become a room section:
they remain the existing Home navigation entry plus Rust-owned
`account_home.invite_count`; `RoomListProjection::Invites` remains authoritative
for consumers of that explicit filter. `SidebarSections.not_joined` contains
only the existing non-invite not-joined room projection. React selects the Rust
`category`, applies only the mounted text filter to that ordered vector, and
renders Rust `collapsed` flags. It does not classify tags/DMs/invites, join rooms
to Spaces, calculate attention, or sort.

### Search highlight projection

Do not add a second command or timeline state machine. `SearchState::Results`
already contains exact Rust-owned `SearchResult.highlights` as UTF-16 ranges.
App indexes those results by `(room_id,event_id)` and passes ranges only when the
Rust result's `match_field` is `messageBody` and its snippet equals the rendered
Rust plain-text body. Timeline rendering may split an admitted range across
sanitized formatted-text nodes; this is presentation mapping of supplied
indices, not matching. Delete `searchHighlight.ts` and every JavaScript
normalization/matcher test.

Before a matching Rust result exists, no inline match is painted. Search result
rows continue to render their Rust ranges as today.

## Legacy WebView Migration

Create one production module, `app/legacyPreferenceMigration.ts`, as the only
allowlisted `localStorage` reader after this PR. It strictly parses the seven
known keys and returns two typed payloads:

1. a partial `SettingsPatch` sent through migration-only
   `ImportLegacySettings` for density, sidebar category/sort/collapse and known
   recent emoji tokens;
2. an optional `NavigationPreferenceUpdate::ImportLegacy` for Home selection
   and Space local presentations.

Ordering:

1. wait for the initial Rust snapshot; navigation import additionally waits for
   the current Ready account and loaded navigation state;
2. submit settings through `ImportLegacySettings` and navigation through its
   typed import variant;
3. wait for the command watermark, persisted import marker and exact authoritative
   snapshot values;
4. remove each corresponding old key only after both marker and value proof;
5. when a marker is already true after a prior crash/remove failure, never
   reapply the old value; remove the stale key after marker proof;
6. retain every unconfirmed key on rejection, persistence failure, account
   replacement or shutdown.

A crash after persistence but before key removal finds the persisted marker and
never reapplies the legacy value, so later Rust edits cannot be overwritten. A
failed `removeItem` leaves the key and fixed private-data-free diagnostic; the
next startup removes it after marker proof without changing Rust data. The compatibility
reader exists only for users upgrading from the localStorage-owning release.
Remove it only after a separately recorded supported-upgrade-window decision;
its structural allowlist and migration tests make that live consumer explicit.

No migration value, Matrix identifier, Space name/icon, DM room ID or recent
emoji list enters diagnostics, Debug output, QA tokens or issue evidence.

## Browser Fixture Deletion

Delete `BrowserFakeApi` rather than split it into smaller product fakes.

- Add an injectable `invoke` function to `TauriDesktopApi`; production defaults
  to Tauri's real `invoke`.
- Reuse `TauriIpcMock` for Vitest/Playwright. Tests install explicit command
  responses and later Rust-shaped snapshot/delta/event fixtures.
- Extract only static snapshot builders needed by tests; they contain data, not
  transitions, comparators, retry rules or command settlement semantics.
- Delete BrowserFake state maps, transition methods and semantic helper modules.
- Delete tests whose only assertion is that TypeScript reproduces a Rust
  reducer/actor. Preserve UI tests by asserting command arguments and then
  injecting the Rust-shaped result that changes the view.
- Production `appRuntime` uses `TauriDesktopApi`; the browser harness installs
  the IPC mock before App boot. A future browser-hosted product must provide a
  real Core/WebWorker adapter, not revive the fake.
- Delete `DesktopApi.setRoomListProjection`; audit every remaining interface
  member and delete any other member with no production caller. Do not replace
  the typed interface with a generic stringly command API.

## Verify-First Gates

Before production edits, add and run these RED checks:

1. Rust settings tests for defaults, legacy JSON backfill, patch normalization,
   stale persistence settlement and settings-store restart.
2. Rust navigation tests for home/Space import, malformed/oversized rejection,
   unknown-Space retention, redacted Debug, encrypted save/load, account
   replacement and stale completion.
3. Rust sidebar tests proving exact section membership/order under all three
   sorts, category-independent section projection, local override projection,
   mute/attention semantics and stable fallback.
4. Browser migration tests seeded with all seven old keys: typed payloads,
   no early removal, exact-snapshot removal, rejection/account-change retention,
   corruption rejection and idempotent replay.
5. Timeline rendering tests proving Rust ranges (including a range crossing
   formatted nodes) and proving no range means no highlight.
6. A repository checker that fails while production localStorage access exists
   outside the migration module, while `searchHighlight.ts`,
   `roomListProjection.ts`, `composeSidebar`/`roomListSections` production
   definitions, or `BrowserFakeApi` exist, or while a fake-only DesktopApi method
   remains.
7. Browser shell/search/composer tests that click/type, assert typed IPC, keep
   the view unchanged after command acceptance alone, then inject a Rust-shaped
   state/event and assert the visible change.

Source guards supplement but never replace behavioral tests.

## Implementation Phases

### Phase A — Canon, schema and migration RED

- Amend overview/state-machine/engineering/state-ownership canon first.
- Add the structural checker and behavioral RED tests.
- Add settings fields, serde defaults and DTO/TypeScript mirrors.

### Phase B — Global settings cutover

- Wire density, sidebar category/sort/collapse and recent emojis to Rust
  snapshots/`UpdateSettings`.
- Delete their normal localStorage owners; leave only migration parsing.
- Prove restart persistence and UI non-optimism.

### Phase C — Encrypted navigation and Space projection

- Add the closed navigation preference command/reducer/store path.
- Project local Space labels/icons in Rust.
- Cut Home and Space presentation authority over; prove data migration.

### Phase D — Sidebar and search semantic deletion

- Ship complete Rust `SidebarSections` ordering.
- Delete TS classification/sort/composition.
- Render only Rust search ranges and delete JS matching.

### Phase E — Browser fake removal

- Inject Tauri invoke, migrate tests to explicit transport fixtures, delete
  BrowserFakeApi and dead DesktopApi members.
- Update `docs/architecture/tauri-react-shell.md`,
  `docs/architecture/frontend-ownership-inventory.md`, and the BrowserFake
  entries in `docs/agents/state-ownership.md` in the same deletion phase; no
  sibling doc may retain the old composition root or semantic-owner claim.
- Keep the browser harness and all user-facing coverage green.

### Phase F — Audit and merge

- Run the full local matrix, preflight review, Fireworks exact-diff review,
  hosted CI, merge, #761 closure and #749 checklist update.

## Verification Matrix

Focused and full evidence, all read by their own exit status:

```bash
cargo fmt --all -- --check
cargo test -p koushi-state --test settings_state --test navigation_state
cargo test -p koushi-core --test runtime_settings
cargo test -p koushi-core --test runtime_core
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
cargo test --workspace
cargo test -p koushi-core --features qa-bin --bin headless-core-qa
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test -- --run
npm --prefix apps/desktop run build
(cd apps/desktop && npx playwright test)
node scripts/check-sdk-submodule.mjs
node scripts/check-agents-docs.mjs
node scripts/check-command-snapshot-contract.mjs
cargo build -p koushi-state --target wasm32-unknown-unknown
cargo deny check
cargo machete
```

Also run generated/wire golden checks, the exact settings/sidebar/search/browser
specs, and relevant local GUI smoke if the rendered shell changes. Hosted merge
gates are all eight CI jobs on the reviewed exact head.

## Design Review Record

- Fireworks `reviewer-flash` round 1 timed out after partial source recon and did
  not clear the gate.
- Focused round 2 found one Important gap (whole-navigation Debug redaction) and
  two Minor ambiguities (invite placement and fresh-store migration eligibility).
- The design now requires complete `NavigationState` redaction with RED evidence,
  keeps invites in Home navigation/count rather than room sections, and permits
  `NotFound` while rejecting only settings load errors.
- Focused round 3 verdict: `CORRECT-TO-IMPLEMENT`; no remaining finding.
- Acceptance-closure amendment review at `cc79c42` found the regular-room
  `PeoplePanel` ladder plus missing App/avatar guard and stale inventory scope.
  The amendment was corrected to add room `role_options`, name every frontend
  retry owner for deletion, and update the inventory; focused re-review verdict:
  `CORRECT-TO-IMPLEMENT`.

## Acceptance-Closure Amendment: Avatar, Sound and Role Policy

Issue #761 also names three later audit mirrors. They close in this PR as a
separate, independently reviewable slice:

1. **Avatar request lifecycle moves fully behind Core after demand submission.**
   React may discover a visible/not-requested MXC and dispatch a typed request,
   but owns no retryability classifier, attempt counter or terminal retry loop.
   `AccountActor` keeps its existing single-flight/in-flight deduplication and
   bounded concurrency, performs at most two network attempts inside the owned
   fetch task, caches both Ready and terminal Failed results for the session,
   and serves later duplicate requests from that cache without another SDK call.
   Session clear aborts tasks and resets the cache as today. Delete
   `MAX_AVATAR_THUMBNAIL_ATTEMPTS`, retryability helpers and retry-count refs from
   `avatarThumbnails.ts`, `TimelineView.tsx`, and App's
   `avatarRetryCountsRef`/`memberAvatarRetryCountsRef`; add a source guard
   against any retry classifier/counter returning. RED tests cover the retry policy, success-after-retry, terminal
   exhaustion and failed-cache reuse.
2. **Desktop notification sound remains an explicit platform-adapter exception.**
   Rust continues to own the authoritative badge count, attention candidate,
   capability facts and user settings. The renderer adapter may retain only the
   positive-edge/cooldown/in-flight mechanics needed to call the webview/window
   sound port. This exception remains covered by `desktopAttention.test.ts` and
   ends when a native Core-owned notification dispatcher replaces that port; it
   must not expand into Matrix attention classification.
3. **Room and Space role choices become uniformly Rust-owned.**
   Space-member DTOs already carry `role_options`; extend regular
   `RoomMemberSummary` with the same Rust-owned option shape, including arbitrary
   current power levels and permission-aware allowed targets. `PeoplePanel`
   renders those options and dispatches the selected numeric level. Delete its
   production `[100, 50, 0]` ladder. Reducer/projection/panel tests and the
   semantic-owner checker pin both room and Space boundaries.

Update `docs/architecture/frontend-ownership-inventory.md` so its avatar row
moves retry/release ownership from renderer to Core and records only visibility
as renderer-owned. Verification adds focused avatar Core tests, removes frontend
retry/count tests, keeps the notification exception tests, checks no production
role constants, and reruns the full Rust/frontend/browser matrix before final
review.

## Non-Goals

- Moving sidebar search text, emoji search text, IME drafts, focus, popovers,
  scroll, DOM measurement or animation to Rust.
- Persisting Matrix identifiers or free-form Space names/icons in
  `settings/settings.json`.
- Adding a generic settings/navigation/request framework.
- Keeping TypeScript semantic code because existing tests depend on it.
- Building the future browser Core/WebWorker adapter in this issue.
- Changing Matrix room/Space names or avatars on the server.
