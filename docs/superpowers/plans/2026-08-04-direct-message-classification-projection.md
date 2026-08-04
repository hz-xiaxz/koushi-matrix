# Direct-message Classification Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore cached DMs in both Home and Space lists at cold start and reclassify them when Sliding Sync updates `m.direct`, without waiting for a new timeline event.

**Architecture:** `koushi-sdk` exposes a local-only, privacy-safe `m.direct` loader and a room normalizer that accepts an explicit direct-room map. The live Room observer subscribes to typed `DirectEvent` updates before reading the local cache, retains one stable direct-classification snapshot, and passes it to every projection. Existing latest-wins Sliding Sync diagnostics carry aggregate classification state to the desktop report.

**Tech Stack:** Rust, Tokio, Matrix Rust SDK typed global account-data events, Matrix SDK UI RoomListService, Koushi actors and diagnostics, TypeScript/Vitest, Tauri macOS DMG tooling.

## Global Constraints

- Sliding Sync is the only supported sync path; do not add legacy `/sync` behavior.
- Room and Space display must not block on a network request for `m.direct`.
- The RoomListService remains the only source of displayed joined/invited rooms.
- `m.direct` is metadata used to classify those rooms, not a second room-list source.
- Home and Space must consume the same `RoomSummary.is_dm` projection.
- Diagnostics may contain only fixed source/reason enums, booleans, and aggregate counts; never Matrix IDs, URLs, tokens, positions, names, or raw errors.
- Preserve every pre-existing dirty-worktree change. Stage new hunks interactively where a touched file already contains unrelated work.

---

## File map

- `crates/koushi-sdk/src/lib.rs`: direct-account-data types, local cache loader, pure map normalization, and explicit-map room snapshot API.
- `crates/koushi-core/src/direct_message_classification.rs`: stable direct-classification state and pure replacement/counting rules.
- `crates/koushi-core/src/room.rs`: race-free DirectEvent observation and use of the stable state for every room projection.
- `crates/koushi-core/src/sliding_sync_diagnostics.rs`: privacy-safe latest-wins DM-classification diagnostics.
- `crates/koushi-core/src/lib.rs`: module registration and exported diagnostic enum.
- `crates/koushi-core/src/account.rs`: pass the shared diagnostics handle into RoomActor.
- `crates/koushi-core/tests/sliding_sync_diagnostics.rs`: diagnostic state and serialization coverage.
- `apps/desktop/src-tauri/src/commands/diagnostics.rs`: expected serialized DTO fields.
- `apps/desktop/src/domain/diagnostics.ts`: TypeScript contract, defaults, and copied-report formatting.
- `apps/desktop/src/domain/diagnostics.test.ts`: copied-report assertions.

---

### Task 1: Local-only direct-account-data SDK API

**Files:**
- Modify: `crates/koushi-sdk/src/lib.rs:8463`
- Modify: `crates/koushi-sdk/src/lib.rs:9840`
- Test: `crates/koushi-sdk/src/lib.rs:11990`

**Interfaces:**
- Produces: `pub type MatrixDirectTargetsByRoom = BTreeMap<String, Vec<String>>`.
- Produces: `pub enum MatrixCachedDirectAccountData { Present(MatrixDirectTargetsByRoom), Missing, StoreError, Invalid }`.
- Produces: `pub async fn cached_direct_account_data_targets_by_room(session: &MatrixClientSession) -> MatrixCachedDirectAccountData`.
- Produces: `pub fn direct_account_data_targets_by_room(content: &DirectEventContent) -> MatrixDirectTargetsByRoom`.
- Produces: `pub async fn room_list_snapshot_from_sdk_rooms_with_direct_targets(rooms: impl IntoIterator<Item = matrix_sdk::Room>, direct_targets_by_room: Option<&MatrixDirectTargetsByRoom>) -> MatrixRoomListSnapshot`.
- Preserves: existing `room_list_snapshot_from_sdk_rooms` and legacy QA helper behavior.

- [ ] **Step 1: Add failing pure-normalization and API-contract tests**

Add these tests beside `direct_account_data_targets_are_indexed_by_room`:

```rust
#[test]
fn direct_account_data_targets_are_sorted_deduplicated_and_indexed_by_room() {
    use matrix_sdk::ruma::{
        OwnedRoomId, OwnedUserId,
        events::direct::{DirectEventContent, OwnedDirectUserIdentifier},
    };

    let alice: OwnedUserId = "@alice:example.invalid".try_into().unwrap();
    let bob: OwnedUserId = "@bob:example.invalid".try_into().unwrap();
    let room: OwnedRoomId = "!dm:example.invalid".try_into().unwrap();
    let mut content = DirectEventContent::default();
    content.insert(OwnedDirectUserIdentifier::from(bob), vec![room.clone()]);
    content.insert(OwnedDirectUserIdentifier::from(alice), vec![room.clone()]);

    assert_eq!(
        super::direct_account_data_targets_by_room(&content),
        BTreeMap::from([(
            room.to_string(),
            vec![
                "@alice:example.invalid".to_owned(),
                "@bob:example.invalid".to_owned(),
            ],
        )]),
    );
}

#[test]
fn live_direct_account_data_loader_is_local_only() {
    let source = include_str!("lib.rs");
    let body = source
        .split("pub async fn cached_direct_account_data_targets_by_room")
        .nth(1)
        .expect("local direct loader")
        .split("pub fn direct_account_data_targets_by_room")
        .next()
        .expect("normalizer follows loader");
    assert!(body.contains("account_data::<DirectEventContent>()"));
    assert!(!body.contains("fetch_account_data_static"));
}
```

- [ ] **Step 2: Run the tests to verify RED**

Run:

```bash
cargo test -p koushi-sdk direct_account_data_targets_are_sorted_deduplicated_and_indexed_by_room -- --nocapture
cargo test -p koushi-sdk live_direct_account_data_loader_is_local_only -- --nocapture
```

Expected: FAIL because the public local-only loader and renamed public normalizer do not exist.

- [ ] **Step 3: Add bounded result types and the local-only loader**

Implement the public types and loader without preserving an SDK error:

```rust
pub type MatrixDirectTargetsByRoom = BTreeMap<String, Vec<String>>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum MatrixCachedDirectAccountData {
    Present(MatrixDirectTargetsByRoom),
    #[default]
    Missing,
    StoreError,
    Invalid,
}

pub async fn cached_direct_account_data_targets_by_room(
    session: &MatrixClientSession,
) -> MatrixCachedDirectAccountData {
    match session
        .client()
        .account()
        .account_data::<DirectEventContent>()
        .await
    {
        Ok(Some(raw)) => match raw.deserialize() {
            Ok(content) => MatrixCachedDirectAccountData::Present(
                direct_account_data_targets_by_room(&content),
            ),
            Err(_) => MatrixCachedDirectAccountData::Invalid,
        },
        Ok(None) => MatrixCachedDirectAccountData::Missing,
        Err(_) => MatrixCachedDirectAccountData::StoreError,
    }
}

pub fn direct_account_data_targets_by_room(
    content: &DirectEventContent,
) -> MatrixDirectTargetsByRoom {
    let mut targets_by_room: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (user_id, room_ids) in content.iter() {
        for room_id in room_ids {
            targets_by_room
                .entry(room_id.to_string())
                .or_default()
                .insert(user_id.to_string());
        }
    }
    targets_by_room
        .into_iter()
        .map(|(room_id, targets)| (room_id, targets.into_iter().collect()))
        .collect()
}
```

Keep the existing network-fallback helper private for legacy one-shot QA callers, but make the live Room observer use only the new local loader.

- [ ] **Step 4: Add the explicit-map room snapshot entry point**

Implement:

```rust
pub async fn room_list_snapshot_from_sdk_rooms_with_direct_targets(
    rooms: impl IntoIterator<Item = matrix_sdk::Room>,
    direct_targets_by_room: Option<&MatrixDirectTargetsByRoom>,
) -> MatrixRoomListSnapshot {
    matrix_room_list_snapshot_from_rooms(direct_targets_by_room, rooms).await
}
```

Change `matrix_room_list_snapshot_from_rooms` to accept
`Option<&MatrixDirectTargetsByRoom>`. With `Some(map)`, set `is_dm` strictly from
`map.contains_key(&room_id)`; with `None`, use the provisional precedence
`room.direct_targets()`, then `room.is_direct().await`/`room.is_dm()`. Pass an
empty map to DM-user resolution only after `is_dm` is false, so stale
`RoomInfo.dm_targets` cannot restore a DM removed from an available map.

Add a focused regression test whose room has cached direct targets but whose
authoritative explicit map is empty; assert the projected room has
`is_dm == false`.

- [ ] **Step 5: Run focused SDK tests to verify GREEN**

Run:

```bash
cargo test -p koushi-sdk direct_account_data_targets -- --nocapture
cargo test -p koushi-sdk joined_room_list_dm_resolution -- --nocapture
cargo test -p koushi-sdk joined_room_list_prefers_async_direct_dm_detection -- --nocapture
```

Expected: all selected tests PASS.

- [ ] **Step 6: Commit the clean SDK-crate change**

```bash
git add crates/koushi-sdk/src/lib.rs
git commit -m "fix: make direct-room metadata an explicit projection input"
```

---

### Task 2: Stable Core classification state and DirectEvent reprojection

**Files:**
- Create: `crates/koushi-core/src/direct_message_classification.rs`
- Modify: `crates/koushi-core/src/lib.rs:37`
- Modify: `crates/koushi-core/src/account.rs:1884`
- Modify: `crates/koushi-core/src/room.rs:296`
- Modify: `crates/koushi-core/src/room.rs:3220`
- Modify: `crates/koushi-core/src/room.rs:3460`
- Modify: `crates/koushi-core/src/room.rs:4057`
- Test: `crates/koushi-core/src/direct_message_classification.rs`
- Test: `crates/koushi-core/src/room.rs:5480`

**Interfaces:**
- Consumes: Task 1 `MatrixCachedDirectAccountData`, `MatrixDirectTargetsByRoom`, `direct_account_data_targets_by_room`, and `room_list_snapshot_from_sdk_rooms_with_direct_targets`.
- Produces: `DirectClassificationState::from_cached`, `targets_by_room`, `replace_from_event`, and `projection_counts`.
- Produces: one DirectEvent observer per live Room observation generation.
- Consumes: a clone of `SlidingSyncDiagnostics` passed to `RoomActor::spawn`.

- [ ] **Step 1: Write failing state tests in the new focused module**

Create `direct_message_classification.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equivalent_event_map_does_not_request_reprojection() {
        let map = MatrixDirectTargetsByRoom::from([(
            "!dm:example.invalid".to_owned(),
            vec!["@alice:example.invalid".to_owned()],
        )]);
        let mut state = DirectClassificationState::from_targets(
            map.clone(),
            DirectAccountDataSource::LocalStore,
        );

        assert!(!state.replace_targets(map));
        assert_eq!(state.event_wake_count(), 1);
        assert_eq!(state.applied_update_count(), 0);
    }

    #[test]
    fn changed_or_removed_mapping_requests_reprojection() {
        let mut state = DirectClassificationState::default();
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::from([(
            "!dm:example.invalid".to_owned(),
            vec!["@alice:example.invalid".to_owned()],
        )])));
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::new()));
        assert_eq!(state.event_wake_count(), 2);
        assert_eq!(state.applied_update_count(), 2);
    }
}
```

- [ ] **Step 2: Verify state tests are RED**

Run:

```bash
cargo test -p koushi-core --lib direct_message_classification::tests -- --nocapture
```

Expected: FAIL because the module and state do not exist.

- [ ] **Step 3: Implement the state boundary**

Implement a small state object; do not put Matrix SDK clients or streams in this file:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectAccountDataSource {
    #[default]
    Unavailable,
    LocalStore,
    SlidingSyncEvent,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DirectClassificationState {
    targets_by_room: MatrixDirectTargetsByRoom,
    source: DirectAccountDataSource,
    invalid_entry_count: u64,
    event_wake_count: u64,
    applied_update_count: u64,
}

impl DirectClassificationState {
    pub(crate) fn from_cached(value: MatrixCachedDirectAccountData) -> Self {
        match value {
            MatrixCachedDirectAccountData::Present(targets_by_room) => {
                Self::from_targets(targets_by_room, DirectAccountDataSource::LocalStore)
            }
            MatrixCachedDirectAccountData::Invalid => Self {
                invalid_entry_count: 1,
                ..Self::default()
            },
            MatrixCachedDirectAccountData::Missing
            | MatrixCachedDirectAccountData::StoreError => Self::default(),
        }
    }
    pub(crate) fn from_targets(
        targets_by_room: MatrixDirectTargetsByRoom,
        source: DirectAccountDataSource,
    ) -> Self {
        Self { targets_by_room, source, ..Self::default() }
    }
    pub(crate) fn targets_by_room(&self) -> &MatrixDirectTargetsByRoom { &self.targets_by_room }
    pub(crate) fn authoritative_targets(&self) -> Option<&MatrixDirectTargetsByRoom> {
        (self.source != DirectAccountDataSource::Unavailable)
            .then_some(&self.targets_by_room)
    }
    pub(crate) fn source(&self) -> DirectAccountDataSource { self.source }
    pub(crate) fn invalid_entry_count(&self) -> u64 { self.invalid_entry_count }
    pub(crate) fn replace_targets(&mut self, next: MatrixDirectTargetsByRoom) -> bool {
        self.event_wake_count = self.event_wake_count.saturating_add(1);
        self.source = DirectAccountDataSource::SlidingSyncEvent;
        if self.targets_by_room == next { return false; }
        self.targets_by_room = next;
        self.applied_update_count = self.applied_update_count.saturating_add(1);
        true
    }
    pub(crate) fn event_wake_count(&self) -> u64 { self.event_wake_count }
    pub(crate) fn applied_update_count(&self) -> u64 { self.applied_update_count }
}
```

Export `DirectAccountDataSource` from `lib.rs`. Map `Missing`, `StoreError`, and
`Invalid` to source `Unavailable`; emit the exact bounded initial reason token
(`missing`, `store_error`, or `invalid`) while the loader result is still in
scope. The state retains `invalid_entry_count = 1` only for `Invalid`.

- [ ] **Step 4: Add a failing observer regression test**

Extend `LiveObserverTestEvent` with:

```rust
DirectClassificationProjected {
    event_wake_count: u64,
    applied_update_count: u64,
    projected_dm_count: usize,
},
```

Add `live_room_list_observer_reclassifies_dm_from_direct_event_without_timeline_update`. Build one joined room with no initial `dm_targets`, start the harness, then feed a typed `DirectEventContent` through a test-only `mpsc::UnboundedReceiver<DirectEventContent>` source accepted by `run_live_room_list_observation_with_sources`. Assert:

```rust
assert!(matches!(
    projected.as_slice(),
    [AppAction::RoomListSnapshotProvisional { rooms, .. },
     AppAction::UserProfilesUpdated { .. }]
        if rooms.iter().any(|room| room.room_id == dm_room_id.as_str() && room.is_dm)
));
harness.expect_event(
    "direct account-data reprojection",
    LiveObserverTestEvent::DirectClassificationProjected {
        event_wake_count: 1,
        applied_update_count: 1,
        projected_dm_count: 1,
    },
).await;
```

Send the same content again and use a 100 ms timeout on `action_rx.recv()` to prove no duplicate projection occurs.

Add `normalize_and_project_entries_uses_cached_direct_map_before_timeline_update`.
Create a joined room through `MatrixMockServer` without direct targets, construct
`DirectClassificationState::from_targets` with that room ID, call
`normalize_and_project_entries`, and assert the first emitted
`RoomListSnapshotProvisional` room has `is_dm == true`. Do not send a timeline
event or a base-room update in this test.

Add this source-order contract test to guard the startup race:

```rust
#[test]
fn live_direct_observer_subscribes_before_cached_account_data_read() {
    let source = include_str!("room.rs");
    let body = source
        .split("async fn run_live_room_list_observation(")
        .nth(1)
        .expect("live observer")
        .split("async fn run_live_room_list_observation_with_sources(")
        .next()
        .expect("wrapper body");
    let subscribe = body.find("observe_events::<DirectEvent, ()>").unwrap();
    let cached_read = body
        .find("cached_direct_account_data_targets_by_room")
        .unwrap();
    assert!(subscribe < cached_read);
}
```

- [ ] **Step 5: Verify observer regression is RED**

Run:

```bash
cargo test -p koushi-core --lib live_room_list_observer_reclassifies_dm_from_direct_event_without_timeline_update -- --nocapture
cargo test -p koushi-core --lib normalize_and_project_entries_uses_cached_direct_map_before_timeline_update -- --nocapture
cargo test -p koushi-core --lib live_direct_observer_subscribes_before_cached_account_data_read -- --nocapture
```

Expected: FAIL because the observer has no DirectEvent input and normalizes with an empty map.

- [ ] **Step 6: Subscribe before the cached read and retain both inputs**

In `run_live_room_list_observation`, create the observer before loading cached account data:

```rust
use matrix_sdk::ruma::events::direct::DirectEvent;

let direct_observer = session.client().observe_events::<DirectEvent, ()>();
let direct_events = direct_observer.subscribe();
let cached_direct = koushi_sdk::cached_direct_account_data_targets_by_room(&session).await;
let direct_state = DirectClassificationState::from_cached(cached_direct);
```

Pass `direct_observer` ownership, the subscriber, and `direct_state` into the production observation loop. Keep the existing test-only injected content receiver as a deterministic alternate source; production must use only the typed SDK subscriber.

- [ ] **Step 7: Pass one explicit map to every projection path**

Add `direct_state: &DirectClassificationState` to
`project_live_entries_and_ack_if_reconciled` and
`direct_targets_by_room: Option<&MatrixDirectTargetsByRoom>` to
`normalize_and_project_entries`. Replace:

```rust
let mut snapshot = koushi_sdk::room_list_snapshot_from_sdk_rooms(joined_rooms).await;
```

with:

```rust
let mut snapshot = koushi_sdk::room_list_snapshot_from_sdk_rooms_with_direct_targets(
    joined_rooms,
    direct_targets_by_room,
).await;
```

Here `direct_targets_by_room` is
`direct_state.authoritative_targets()`: `Some(empty_map)` after an authoritative
empty DirectEvent, and `None` only while no valid `m.direct` has been loaded.

Update every call from commands, loading/range state, RoomListService diffs, base room updates, and reconciliation. There must be no call in the live observer that supplies `BTreeMap::new()` or performs a network fetch.

- [ ] **Step 8: Add the DirectEvent select branch**

Normalize event content, compare, and project only on change:

```rust
next_direct = direct_events.next(), if !direct_events_closed => {
    match next_direct {
        Some((event, ())) => {
            let changed = direct_state.replace_targets(
                koushi_sdk::direct_account_data_targets_by_room(&event.content),
            );
            if changed {
                project_live_entries_and_ack_if_reconciled(
                    &mut reconciliation,
                    &session,
                    &current,
                    &direct_state,
                    &known_room_ids,
                    &room_tx,
                    &action_tx,
                    &event_tx,
                    generation,
                    source,
                    &authoritative,
                ).await;
            }
        }
        None => {
            direct_events_closed = true;
            record_direct_event_stream_closed(&direct_state);
        }
    }
}
```

Do not break the loop when this auxiliary stream closes. The DirectEvent observer object must remain in scope for as long as its subscriber.

- [ ] **Step 9: Update RoomActor construction**

Change the constructor to:

```rust
pub fn spawn(
    action_tx: mpsc::Sender<Vec<AppAction>>,
    event_tx: broadcast::Sender<CoreEvent>,
    sliding_sync_diagnostics: crate::SlidingSyncDiagnostics,
) -> RoomActorHandle
```

Pass `sliding_sync_diagnostics.clone()` from `AccountActor::spawn_with_diagnostics`, and pass fresh defaults at direct unit-test construction sites. Retain the clone in `RoomActor` and pass it to each observation generation.

- [ ] **Step 10: Verify Core behavior is GREEN**

Run:

```bash
cargo test -p koushi-core --lib direct_message_classification::tests -- --nocapture
cargo test -p koushi-core --lib live_room_list_observer_reclassifies_dm_from_direct_event_without_timeline_update -- --nocapture
cargo test -p koushi-core --lib live_room_list_observer_projects_rooms_and_invites_from_service_entries -- --nocapture
cargo test -p koushi-core --lib live_projection_does_not_import_base_client_only_invites -- --nocapture
```

Expected: all selected tests PASS; the invite test confirms `m.direct` metadata did not become a second room source.

- [ ] **Step 11: Stage only Task 2 hunks and commit**

Because `room.rs`, `account.rs`, and `lib.rs` already contain intentional work, inspect and stage only this task's hunks:

```bash
git diff -- crates/koushi-core/src/direct_message_classification.rs crates/koushi-core/src/room.rs crates/koushi-core/src/account.rs crates/koushi-core/src/lib.rs
git add crates/koushi-core/src/direct_message_classification.rs
git add -p crates/koushi-core/src/room.rs crates/koushi-core/src/account.rs crates/koushi-core/src/lib.rs
git diff --cached --check
git commit -m "fix: reproject direct rooms from sliding sync account data"
```

---

### Task 3: Latest-wins privacy-safe diagnostics

**Files:**
- Modify: `crates/koushi-core/src/sliding_sync_diagnostics.rs`
- Modify: `crates/koushi-core/src/lib.rs:78`
- Modify: `crates/koushi-core/src/room.rs:4057`
- Test: `crates/koushi-core/tests/sliding_sync_diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/commands/diagnostics.rs:19`
- Modify: `apps/desktop/src/domain/diagnostics.ts:16`
- Test: `apps/desktop/src/domain/diagnostics.test.ts`

**Interfaces:**
- Consumes: Task 2 `DirectAccountDataSource::{Unavailable, LocalStore, SlidingSyncEvent}`, already serialized as snake case.
- Extends: `SlidingSyncDiagnosticsSnapshot` with current source and aggregate counters.
- Consumes: Task 2 projection counts and event wake/applied counts.
- Produces copied-report keys prefixed by `direct_classification.`.

- [ ] **Step 1: Write failing Rust diagnostics tests**

Add to `crates/koushi-core/tests/sliding_sync_diagnostics.rs`:

```rust
#[test]
fn direct_classification_diagnostics_are_latest_wins() {
    let diagnostics = SlidingSyncDiagnostics::default();
    diagnostics.direct_classification_initialized(
        DirectAccountDataSource::LocalStore, 3, 4,
    );
    diagnostics.direct_projection_recorded(2, 1, 1, 7, 0);
    diagnostics.direct_event_recorded(
        DirectAccountDataSource::SlidingSyncEvent, 4, 5, 1, 1, true,
    );

    let snapshot = diagnostics.snapshot();
    assert_eq!(snapshot.direct_account_data_source, DirectAccountDataSource::SlidingSyncEvent);
    assert_eq!(snapshot.direct_mapped_room_count, 4);
    assert_eq!(snapshot.direct_target_count, 5);
    assert_eq!(snapshot.projected_dm_count, 2);
    assert_eq!(snapshot.explicit_dm_count, 1);
    assert_eq!(snapshot.fallback_dm_count, 1);
    assert_eq!(snapshot.direct_non_dm_count, 7);
    assert_eq!(snapshot.direct_invalid_entry_count, 0);
    assert_eq!(snapshot.direct_event_wake_count, 1);
    assert_eq!(snapshot.direct_event_applied_count, 1);
    assert!(snapshot.direct_event_stream_running);
}
```

Extend the serialization privacy test to call these typed methods and retain the existing forbidden-value assertions.

- [ ] **Step 2: Verify Rust diagnostics RED**

Run:

```bash
cargo test -p koushi-core --test sliding_sync_diagnostics direct_classification -- --nocapture
```

Expected: FAIL because the snapshot fields and update methods do not exist.

- [ ] **Step 3: Implement fixed-type diagnostics fields**

Add these fields to `SlidingSyncDiagnosticsSnapshot` and defaults:

```rust
pub direct_account_data_source: DirectAccountDataSource,
pub direct_mapped_room_count: u64,
pub direct_target_count: u64,
pub projected_dm_count: u64,
pub explicit_dm_count: u64,
pub fallback_dm_count: u64,
pub direct_non_dm_count: u64,
pub direct_invalid_entry_count: u64,
pub direct_event_wake_count: u64,
pub direct_event_applied_count: u64,
pub direct_event_stream_running: bool,
```

Implement methods with only enums and `u64`/`bool` arguments:

```rust
pub fn direct_classification_initialized(
    &self, source: DirectAccountDataSource, mapped_rooms: u64, targets: u64,
);
pub fn direct_event_recorded(
    &self, source: DirectAccountDataSource, mapped_rooms: u64, targets: u64,
    wakes: u64, applied: u64, stream_running: bool,
);
pub fn direct_projection_recorded(
    &self, projected_dms: u64, explicit_dms: u64, fallback_dms: u64,
    non_dms: u64, invalid_entries: u64,
);
```

The `non_dms` and `invalid_entries` arguments must also be retained in snapshot fields so the diagnostic report covers all classification outcomes promised by the spec.

- [ ] **Step 4: Record initialization, updates, projection counts, and closure**

In Room observation:

- after the cached read, record source, mapped-room count, and total target count;
- after every DirectEvent, record wake/applied counts even when the map is unchanged;
- after every completed normalization, count explicit DMs by map membership, fallback DMs as projected DMs minus explicit DMs, and non-DMs from projected non-space rooms;
- when the DirectEvent stream closes, set `direct_event_stream_running = false` and emit one bounded `core.room stage=direct_event_stream_closed` diagnostic event.

Use saturating conversions to `u64`; do not format IDs or raw errors.

- [ ] **Step 5: Add failing desktop contract/report assertions**

Extend `SlidingSyncDiagnostics` and `DEFAULT_SLIDING_SYNC_DIAGNOSTICS` with camelCase equivalents. Assert `diagnosticReport` contains:

```text
direct_classification.source=sliding_sync_event
direct_classification.mapped_room_count=4
direct_classification.target_count=5
direct_classification.projected_dm_count=2
direct_classification.explicit_dm_count=1
direct_classification.fallback_dm_count=1
direct_classification.non_dm_count=7
direct_classification.invalid_entry_count=0
direct_classification.event_wake_count=1
direct_classification.event_applied_count=1
direct_classification.event_stream_running=true
```

Also extend the Tauri JSON equality test with the corresponding default values.

- [ ] **Step 6: Verify desktop tests are RED**

Run:

```bash
cargo test -p koushi-desktop diagnostic_snapshot_maps_structured_snapshot_to_camel_case_frontend_contract -- --nocapture
npm --prefix apps/desktop test -- src/domain/diagnostics.test.ts
```

Expected: FAIL until Rust serialization, TypeScript contract, defaults, and formatting agree.

- [ ] **Step 7: Implement desktop formatting and verify GREEN**

Append the fixed keys to `formatSlidingSyncDiagnostics`, using `Math.max(0, Math.trunc(...))` for every count. Then run:

```bash
cargo test -p koushi-core --test sliding_sync_diagnostics -- --nocapture
cargo test -p koushi-desktop diagnostics -- --nocapture
npm --prefix apps/desktop test -- src/domain/diagnostics.test.ts
npm --prefix apps/desktop run typecheck
```

Expected: all commands exit 0.

- [ ] **Step 8: Stage only diagnostics hunks and commit**

```bash
git add -p crates/koushi-core/src/sliding_sync_diagnostics.rs crates/koushi-core/src/lib.rs crates/koushi-core/src/room.rs apps/desktop/src-tauri/src/commands/diagnostics.rs apps/desktop/src/domain/diagnostics.ts apps/desktop/src/domain/diagnostics.test.ts
git add -p crates/koushi-core/tests/sliding_sync_diagnostics.rs
git diff --cached --check
git commit -m "feat: diagnose direct-message classification state"
```

---

### Task 4: Regression verification and fast local DMG

**Files:**
- Verify: all Task 1-3 files
- Build: `target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`
- Copy: `/Users/hiroshi/projects/Element-dev/matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg`

**Interfaces:**
- Produces: an installable arm64 DMG containing the startup DM-classification fix and expanded diagnostics.
- Preserves: room/space restoration, invite projection, message send/receive, and existing dirty-worktree fixes.

- [ ] **Step 1: Format and inspect the complete diff**

Run:

```bash
cargo fmt --all
git diff --check
git status --short
git diff --submodule=short --stat
```

Expected: no whitespace errors; only intentional files remain modified/untracked. Do not discard or overwrite pre-existing work.

- [ ] **Step 2: Run the fast regression set**

Run:

```bash
cargo test -p koushi-sdk direct_account_data_targets -- --nocapture
cargo test -p koushi-core --lib direct_message_classification::tests -- --nocapture
cargo test -p koushi-core --lib live_room_list_observer -- --nocapture
cargo test -p koushi-core --test sliding_sync_diagnostics -- --nocapture
cargo test -p koushi-desktop diagnostics -- --nocapture
npm --prefix apps/desktop test -- src/domain/diagnostics.test.ts
```

Expected: all selected tests PASS. Broad unit suites are intentionally deferred until after the user validates the DMG.

- [ ] **Step 3: Build the DMG without broad preflight**

Run:

```bash
npm --prefix apps/desktop run build:dmg -- --skip-preflight
```

Expected: command exits 0 and reports a DMG under `target/release/bundle/dmg/`.

- [ ] **Step 4: Verify and copy the artifact**

Run:

```bash
hdiutil verify target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
shasum -a 256 target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
cp target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg /Users/hiroshi/projects/Element-dev/matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
shasum -a 256 /Users/hiroshi/projects/Element-dev/matrix-desktop/target/release/bundle/dmg/Koushi_0.1.0_aarch64.dmg
```

Expected: `hdiutil` reports `VALID`; source and copied SHA-256 values match.

- [ ] **Step 5: User cold-start validation**

Install the DMG without deleting the existing database. Before asking the missing counterpart to send anything, confirm:

1. the historical DM is present in Home;
2. the same DM is present in every applicable Space;
3. normal Rooms and Spaces are still present;
4. copied diagnostics report `direct_classification.source`, mapped-room count, projected-DM count, and DirectEvent counters.

If the DM is still absent, collect one diagnostic report before any new message. Do not add another timing-based repair without locating the first mismatched aggregate count.

---

### Task 5: Formal branch verification and ready PR after local validation

**Files:**
- Review: complete branch diff against `origin/main`

**Interfaces:**
- Produces: one ready-for-review PR containing the previously tested Sliding Sync restoration work plus this DM-classification correction.

- [ ] **Step 1: Run broader affected suites**

```bash
cargo test -p koushi-sdk --lib
cargo test -p koushi-core --lib
cargo test -p koushi-core --test sliding_sync_diagnostics
cargo test -p koushi-desktop
npm --prefix apps/desktop test
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
```

Expected: all commands exit 0. Record any unrelated pre-existing failure separately with its exact command and output.

- [ ] **Step 2: Run repository safety checks**

```bash
npm --prefix apps/desktop run guard:sdk
npm --prefix apps/desktop run qa:secret-scan
git diff --check origin/main...HEAD
git status --short
```

Expected: SDK gitlink policy and secret scan pass; no accidental artifacts or private diagnostic values are staged.

- [ ] **Step 3: Reconcile intentional dirty changes into commits**

Review every remaining diff against the already validated DMG. Stage by logical scope with `git add -p`; never use `git add -A`. Commit only changes that were present in the tested artifact, using messages that describe the Sliding Sync restoration, diagnostics, and DM-classification scopes.

- [ ] **Step 4: Push and open a ready PR**

```bash
git push -u origin codex/sliding-sync-runtime-diagnostics
gh pr create --base main --head codex/sliding-sync-runtime-diagnostics --title "Fix Sliding Sync restoration and startup DM classification" --body "$(git log --format='- %s' origin/main..HEAD)"
gh pr ready
```

The PR body must list the cold-start DM reproduction, the explicit `m.direct` projection fix, privacy-safe diagnostic additions, DMG SHA-256, focused and broad test results, and any intentionally deferred platform QA.
