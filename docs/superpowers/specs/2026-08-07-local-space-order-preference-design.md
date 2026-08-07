# Local Space Order Preference Design

Date: 2026-08-07

## Summary

Koushi will treat Space ordering as an account-scoped, device-local user preference rather than as a cache of the currently visible Space list. Sliding Sync room-list snapshots may change which Spaces are visible, but they must not delete or reorder the user's persisted preference. A Space is removed from the preference only after an explicit leave operation succeeds or the account's local data is erased.

This design fixes the startup regression in which a persisted order is loaded before the room list, reconciled against an empty `state.spaces`, discarded, and later overwritten with server arrival order.

## Goals

- Restore the user's local Space order after every restart, independent of startup timing.
- Preserve positions across provisional, incomplete, or transiently empty Sliding Sync projections.
- Restore a temporarily missing Space to its previous position when it reappears.
- Append newly joined or newly discovered Spaces deterministically.
- Preserve hidden Space entries when the user reorders only currently visible Spaces.
- Remove an entry only after explicit leave success or account-local data deletion.
- Make persistence failures visible in diagnostics instead of silently replacing state with defaults.
- Keep the existing encrypted navigation data readable without migration.

## Non-goals

- Synchronizing Space order between devices or with Element.
- Writing Matrix account data for the ordering preference.
- Defining server-side Space ordering semantics.
- Solving the separate root cause of Spaces temporarily disappearing from Sliding Sync projections.
- Adding a user-facing preference editor beyond the existing drag-and-drop rail.

## Current Failure

The current flow is:

1. Session restoration makes an account key available.
2. `load_navigation_for_current_session` loads the encrypted `NavigationState` before Sliding Sync has populated `state.spaces`.
3. `handle_navigation_loaded` calls `reconcile_space_order` against the empty Space list.
4. `reconcile_space_order` removes every persisted Space ID because none is currently available.
5. The first room-list snapshot appends Spaces in snapshot arrival order.
6. Runtime navigation persistence writes that replacement order back to disk.

The existing reducer test loads navigation after pre-populating `state.spaces`, so it does not represent the real startup sequence.

## State Ownership

`NavigationState.space_order` remains the persisted field for compatibility, but its contract changes. It becomes the preferred-order ledger for the account on this device.

The ledger has these invariants:

1. Each Space ID appears at most once.
2. Missing visibility is not evidence of removal.
3. Existing entries retain relative order unless the user reorders visible Spaces.
4. A newly observed Space absent from the ledger is appended once.
5. Only explicit leave success removes one Space entry.
6. Account-local data deletion removes the entire ledger with the rest of navigation state.

There is no separately persisted `visible_space_order`. The ordered `state.spaces` projection is derived from the ledger and the current incoming Space summaries.

## Ordering Operations

Replace the destructive, multi-purpose `reconcile_space_order` operation with explicit operations whose names describe whether they may mutate the preference.

### Normalize a loaded ledger

`normalize_space_order_preference` removes duplicate or structurally invalid entries while preserving first occurrence order. It does not inspect current visibility and therefore works before room-list initialization.

Conceptual signature:

```rust
fn normalize_space_order_preference(space_order: &mut Vec<String>) -> NormalizeResult;
```

### Merge newly observed Spaces

`merge_new_spaces_into_preference` appends incoming Space IDs that are not already in the ledger. It never removes entries.

```rust
fn merge_new_spaces_into_preference(
    space_order: &mut Vec<String>,
    incoming_spaces: &[SpaceSummary],
) -> SpaceOrderMergeResult;
```

`SpaceOrderMergeResult` reports at least:

- `newly_appended_count`
- `visible_count`
- `temporarily_absent_count`
- whether the ledger changed and therefore needs persistence

Incoming order is used only to order previously unknown Spaces appended during the same merge. It cannot reorder known entries.

### Project visible Spaces

`apply_space_order_preference` sorts only the incoming visible summaries. Entries in the ledger that are not visible remain in the ledger but do not produce rail items.

```rust
fn apply_space_order_preference(
    spaces: &mut [SpaceSummary],
    space_order: &[String],
);
```

All visible Spaces should already be present after the merge step. A defensive fallback places an unexpected unlisted Space at the end without mutating the ledger inside the projection function.

### Apply a visible-only reorder

The desktop sends the currently visible Space IDs. Assigning this array directly to the ledger would delete temporarily absent entries. Instead, validate that the request is an exact permutation of the currently visible IDs and replace only visible slots in the existing ledger.

Example:

```text
ledger before:    B, C, A, D
currently visible B, A, D
requested order:  D, B, A
ledger after:     D, C, B, A
```

Conceptual signature:

```rust
fn apply_visible_space_reorder(
    space_order: &mut Vec<String>,
    currently_visible: &[SpaceSummary],
    requested_visible_order: &[String],
) -> Result<SpaceOrderReorderResult, SpaceOrderReorderError>;
```

The algorithm is:

1. Verify that `requested_visible_order` has no duplicates.
2. Verify that it contains exactly the IDs in `currently_visible`.
3. Ensure all visible IDs exist in the ledger by running the merge operation first.
4. Scan ledger positions from left to right.
5. Leave absent-ID positions unchanged.
6. Replace each visible-ID position with the next ID from the requested order.
7. Persist only when the resulting ledger differs.

Rejected requests do not mutate memory or disk and produce a diagnostic reason.

### Remove after explicit leave

Room-list absence never calls removal. The successful completion of an explicit Space leave operation emits this dedicated action:

```rust
AppAction::SpaceOrderPreferenceRemoved {
    space_id,
    reason: SpaceOrderRemovalReason::ExplicitLeave,
}
```

The reducer removes the ID and persists the resulting ledger. Failed, cancelled, timed-out, or outcome-unknown leave operations do not remove it. If a late success is observed after an unknown outcome, the terminal success action may remove it then.

## Data Flow

### Startup

```text
restore session
  -> load encrypted NavigationState
  -> normalize persisted ledger without consulting state.spaces
  -> retain ledger while room list is empty
  -> receive provisional or authoritative Space summaries
  -> append only genuinely new IDs
  -> derive visible order from ledger
  -> persist only if new IDs were appended
```

`navigation_loaded_for` may continue preventing repeated store loads. Correctness no longer depends on loading after Spaces arrive.

### Room-list updates

Both provisional and authoritative updates use the same non-destructive preference behavior:

```text
incoming Spaces
  -> merge unknown IDs into ledger
  -> apply ledger order to incoming visible summaries
  -> update state.spaces
```

No room-list source, generation, or readiness state is allowed to remove ledger entries. This is deliberate defense against an authoritative projection that is temporarily incomplete.

### User reorder

```text
drag/drop rail
  -> requested permutation of visible IDs
  -> validate against current visible set
  -> replace visible ledger slots
  -> update state.spaces projection
  -> persist full navigation state
```

Keyboard and future accessibility reorder controls must call the same command.

## Persistence and Failure Handling

The existing encrypted per-account navigation file remains the storage format. Keeping the serialized `spaceOrder` field avoids a migration and lets old DMGs and the new implementation read the same data.

Navigation writes should use the store's atomic-file pattern: write the complete encrypted payload to a sibling temporary file, flush it, and rename it over the destination. The runtime already serializes navigation mutations through its action loop, so a second parallel writer is unnecessary.

The runtime tracks navigation persistence health independently of the reducer state:

```rust
enum NavigationPersistenceHealth {
    Unloaded,
    Loaded,
    Missing,
    LoadFailed,
}
```

Room-list projections may request persistence only when health is `Loaded` or `Missing`. This prevents a corrupt or temporarily unreadable navigation file from being overwritten by server arrival order. After `LoadFailed`, the first explicit user preference mutation must quarantine the unreadable file beside the navigation file, atomically write the complete current state, and move health to `Loaded`. If quarantine or replacement fails, the original file remains untouched and health remains `LoadFailed`.

The following silent fallbacks must be removed from the navigation path:

- A load error must not be indistinguishable from a missing file.
- A save error must not be discarded by `let _ = ...`.

Behavior by failure type:

- Missing file: load the default ledger and record a normal `not_found` outcome.
- Decryption, format, or read failure: retain safe in-memory defaults, set persistence health to `LoadFailed`, emit an error diagnostic, and suppress projection-derived writes.
- Save failure: keep the current in-memory order, emit an error diagnostic, and retry by writing the latest complete navigation state on the next navigation mutation.
- Invalid reorder request: preserve current state and return a structured rejection.

A timer-based retry is out of scope; latest-state retry on the next navigation mutation is sufficient for this change.

## Diagnostics

Add structured events under the source `core.space_order`.

Events:

- `load_completed`
- `load_failed`
- `projection_applied`
- `preference_extended`
- `reorder_applied`
- `reorder_rejected`
- `preference_removed`
- `save_completed`
- `save_failed`

Fields should include:

- account-safe correlation token, following existing diagnostic redaction policy
- room-list source and generation where applicable
- `preferred_count`
- `visible_count`
- `temporarily_absent_count`
- `newly_appended_count`
- `explicitly_removed_count`
- stable digest of the ordered IDs before and after
- rejection or persistence failure kind

Do not emit raw Space IDs as an unbounded diagnostic list. A bounded example ID may be included only if existing diagnostic privacy conventions permit it.

## Compatibility

- Existing encrypted navigation files remain readable because `spaceOrder` keeps its serialized name and type.
- No Matrix protocol or homeserver support change is required.
- The ordering remains local to the device and account.
- Existing frontend DTOs may continue exposing `space_order`; only its contract changes.
- The hidden `space_order` entries are not sent to the frontend rail unless their Space is currently visible.

## Test Plan

### Reducer tests

1. Load `[B, A]` while `state.spaces` is empty and assert the ledger remains `[B, A]`.
2. Apply incoming `[A, B]` and assert visible `state.spaces` becomes `[B, A]`.
3. Apply provisional and authoritative updates missing `B`; assert the ledger remains `[B, A]`.
4. Reintroduce `B`; assert it returns before `A`.
5. Introduce `C`; assert the ledger becomes `[B, A, C]` exactly once.
6. Reorder visible IDs while one ledger entry is absent; assert the absent entry remains in its slot.
7. Reject duplicate, incomplete, unknown, and stale visible reorder requests without mutation.
8. Remove a Space after explicit leave success and no other event.

### Store tests

1. Save and reload a ledger with visible and absent entries.
2. Load an existing encrypted navigation fixture without migration.
3. Distinguish missing, corrupt, and unreadable files.
4. Verify a failed replacement does not destroy the previous valid file.
5. Verify recovery after `LoadFailed` quarantines the unreadable file before installing a valid replacement.

### Runtime integration tests

1. Persist `[B, A]`, restart runtime, restore the session before room-list delivery, then deliver `[A, B]`; assert both snapshot and disk retain `[B, A]`.
2. Deliver a transient empty or partial projection followed by recovery; assert order continuity.
3. Verify newly appended IDs trigger one persistence update while visibility-only changes do not.
4. Verify save and load failures produce diagnostic records.
5. Verify a load failure suppresses projection-derived writes until an explicit user preference mutation successfully recovers persistence.

### Desktop test

Exercise drag-and-drop with three visible Spaces, restart the packaged-like harness, and assert the rail order is restored. Add a harness case containing a hidden ledger entry so frontend submission cannot accidentally truncate it.

## Acceptance Criteria

- A user-selected Space order survives process restart when navigation loads before the room list.
- Empty, partial, provisional, and authoritative projections cannot delete preference entries.
- A temporarily missing Space returns to its previous position.
- New Spaces appear at the end and remain reorderable.
- Reordering visible Spaces preserves hidden entries.
- Explicit leave success removes the corresponding preference entry.
- Existing encrypted navigation data requires no migration.
- Navigation load and save failures are diagnosable and are not silently treated as successful defaults.
- An unreadable navigation file is not overwritten by startup room-list projections.

## Implementation Boundaries

The implementation should be limited to:

- Space-order helper semantics in `koushi-state`
- reducer actions for merge, reorder, and explicit removal
- runtime navigation persistence error handling and diagnostics
- atomic navigation store writes
- Tauri/frontend contract tests needed to preserve visible-only reorder behavior

Unrelated room-list continuity, Space discovery, and cross-device preference work must remain separate.
